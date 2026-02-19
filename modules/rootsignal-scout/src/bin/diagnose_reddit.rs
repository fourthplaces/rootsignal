//! Diagnostic tool: trace r/Minneapolis posts through the FULL pipeline.
//! Shows what Apify returns, what the LLM extracts, and what survives each
//! stage: quality scoring → geo-filter → title dedup → embedding dedup.
//!
//! Usage: cargo run --bin diagnose_reddit

use anyhow::Result;
use apify_client::ApifyClient;
use rootsignal_common::{Node, NodeType};
use rootsignal_graph::GraphClient;
use rootsignal_scout::embedder::Embedder;
use rootsignal_scout::extractor::{Extractor, SignalExtractor};
use rootsignal_scout::quality;
use rootsignal_scout::scraper::{SocialAccount, SocialPlatform, SocialPost, SocialScraper};

const SUBREDDIT_URL: &str = "https://www.reddit.com/r/Minneapolis/";
const POST_LIMIT: u32 = 20;

// City config (from graph)
const CENTER_LAT: f64 = 44.9773;
const CENTER_LNG: f64 = -93.2655;
const RADIUS_KM: f64 = 30.0;
const GEO_TERMS: &[&str] = &["Minneapolis", "Minnesota"];

#[tokio::main]
async fn main() -> Result<()> {
    dotenv_load();
    let apify_key = std::env::var("APIFY_API_KEY").expect("APIFY_API_KEY required");
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY required");
    let voyage_key = std::env::var("VOYAGE_API_KEY").expect("VOYAGE_API_KEY required");

    // Connect to Neo4j for dedup checks
    let graph = GraphClient::connect("bolt://localhost:7687", "neo4j", "rootsignal").await?;
    let writer = rootsignal_graph::GraphWriter::new(graph);

    // ================================================================
    // STAGE 1: Apify
    // ================================================================
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  STAGE 1: Apify Reddit Scrape                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let apify = ApifyClient::new(apify_key);
    let account = SocialAccount {
        platform: SocialPlatform::Reddit,
        identifier: SUBREDDIT_URL.to_string(),
    };

    let posts: Vec<SocialPost> = apify.search_posts(&account, POST_LIMIT).await?;
    println!("Apify returned {} posts\n", posts.len());

    // Analyze what we got
    let mut unique_threads: std::collections::HashSet<String> = std::collections::HashSet::new();
    for post in &posts {
        if let Some(ref url) = post.url {
            // Extract thread URL (strip comment suffix)
            let thread = if let Some(idx) = url.find("/comments/") {
                let rest = &url[idx + "/comments/".len()..];
                if let Some(slash_idx) = rest.find('/') {
                    let after_id = &rest[slash_idx + 1..];
                    if let Some(next_slash) = after_id.find('/') {
                        // Has comment ID suffix — this is a comment
                        url[..idx + "/comments/".len() + slash_idx + 1 + next_slash + 1].to_string()
                    } else {
                        url.clone()
                    }
                } else {
                    url.clone()
                }
            } else {
                url.clone()
            };
            unique_threads.insert(thread);
        }
    }

    let comment_count = posts.len() - unique_threads.len();
    println!("  → {} unique threads, ~{} are comments\n", unique_threads.len(), comment_count);

    for (i, post) in posts.iter().enumerate() {
        let url = post.url.as_deref().unwrap_or("(no url)");
        let is_comment = url.matches('/').count() > 8; // rough heuristic
        let tag = if is_comment { " [COMMENT]" } else { "" };
        let preview: String = post.content.chars().take(100).collect();
        println!("  {:>2}. ({:>4} chars){} {}", i + 1, post.content.len(), tag, preview);
    }
    println!();

    if posts.is_empty() {
        println!("No posts returned. Done.");
        return Ok(());
    }

    // ================================================================
    // STAGE 2: LLM Extraction
    // ================================================================
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  STAGE 2: LLM Extraction (Claude Haiku)                    ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let extractor = Extractor::new(&anthropic_key, "Minneapolis", CENTER_LAT, CENTER_LNG);
    let mut all_nodes: Vec<Node> = Vec::new();
    let mut all_combined_text = String::new();

    let batches: Vec<&[SocialPost]> = posts.chunks(10).collect();
    for (batch_idx, batch) in batches.iter().enumerate() {
        let combined_text: String = batch
            .iter()
            .enumerate()
            .map(|(i, p)| match &p.url {
                Some(url) => format!("--- Post {} ({}) ---\n{}", i + 1, url, p.content),
                None => format!("--- Post {} ---\n{}", i + 1, p.content),
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        println!("  Batch {} ({} posts, {} chars)...", batch_idx + 1, batch.len(), combined_text.len());

        match extractor.extract(&combined_text, SUBREDDIT_URL).await {
            Ok(nodes) => {
                println!("  → {} signals extracted\n", nodes.len());
                for (j, node) in nodes.iter().enumerate() {
                    let meta = node.meta().unwrap();
                    println!("    {:>2}. [{:?}] \"{}\"", j + 1, node.node_type(), meta.title);
                    println!("        loc_name={:?}  lat={:?}  sensitivity={:?}",
                        meta.location_name, meta.location.as_ref().map(|l| l.lat), meta.sensitivity);
                    if let Node::Tension(t) = node {
                        println!("        severity={:?}  what_would_help={}", t.severity, trunc(t.what_would_help.as_deref().unwrap_or("none"), 80));
                    }
                }
                println!();
                all_nodes.extend(nodes);
                all_combined_text.push_str(&combined_text);
            }
            Err(e) => println!("  → EXTRACTION FAILED: {}\n", e),
        }
    }

    let total_extracted = all_nodes.len();
    println!("  TOTAL EXTRACTED: {} signals\n", total_extracted);

    // ================================================================
    // STAGE 3: Quality Scoring
    // ================================================================
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  STAGE 3: Quality Scoring                                  ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    for node in &mut all_nodes {
        let q = quality::score(node);
        if let Some(meta) = node_meta_mut(node) {
            meta.confidence = q.confidence;
        }
    }

    for (i, node) in all_nodes.iter().enumerate() {
        let meta = node.meta().unwrap();
        println!("  {:>2}. [{:?}] conf={:.3} \"{}\"",
            i + 1, node.node_type(), meta.confidence, trunc(&meta.title, 60));
    }
    println!();

    // ================================================================
    // STAGE 4: Geo-Filter
    // ================================================================
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  STAGE 4: Geo-Filter                                       ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let is_city_local = true; // This source URL is a known city source
    let mut geo_passed = Vec::new();
    let mut geo_killed = Vec::new();

    for mut node in all_nodes {
        let has_coords = node.meta().and_then(|m| m.location.as_ref()).is_some();
        let loc_name = node.meta().and_then(|m| m.location_name.as_deref()).unwrap_or("").to_string();

        if has_coords {
            let loc = node.meta().unwrap().location.as_ref().unwrap();
            let dist = rootsignal_common::haversine_km(CENTER_LAT, CENTER_LNG, loc.lat, loc.lng);
            if dist <= RADIUS_KM {
                println!("  ✓ PASS (coords in radius, {:.1}km) \"{}\"", dist, node.meta().unwrap().title);
                geo_passed.push(node);
            } else {
                println!("  ✗ KILLED (coords outside radius, {:.1}km) \"{}\"", dist, node.meta().unwrap().title);
                geo_killed.push(node.meta().unwrap().title.clone());
            }
        } else if !loc_name.is_empty() && loc_name != "<UNKNOWN>" {
            let loc_lower = loc_name.to_lowercase();
            if GEO_TERMS.iter().any(|term| loc_lower.contains(&term.to_lowercase())) {
                println!("  ✓ PASS (loc_name matches geo_term) loc=\"{}\" \"{}\"", loc_name, node.meta().unwrap().title);
                geo_passed.push(node);
            } else if is_city_local {
                println!("  ~ PASS with 0.8x penalty (city-local, no geo_term match) loc=\"{}\" \"{}\"", loc_name, node.meta().unwrap().title);
                if let Some(meta) = node_meta_mut(&mut node) {
                    meta.confidence *= 0.8;
                }
                geo_passed.push(node);
            } else {
                println!("  ✗ KILLED (non-local, no geo_term match) loc=\"{}\" \"{}\"", loc_name, node.meta().unwrap().title);
                geo_killed.push(node.meta().unwrap().title.clone());
            }
        } else {
            println!("  ✓ PASS (no coords, no loc_name — benefit of doubt) \"{}\"", node.meta().unwrap().title);
            geo_passed.push(node);
        }
    }

    println!("\n  Geo result: {} passed, {} killed\n", geo_passed.len(), geo_killed.len());

    // ================================================================
    // STAGE 5: Within-Batch Title Dedup
    // ================================================================
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  STAGE 5: Within-Batch Title Dedup                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let mut seen = std::collections::HashSet::new();
    let mut title_dedup_passed = Vec::new();
    let mut title_dedup_killed = Vec::new();

    for node in geo_passed {
        let key = (node.meta().unwrap().title.trim().to_lowercase(), node.node_type());
        if seen.insert(key) {
            println!("  ✓ PASS \"{}\"", node.meta().unwrap().title);
            title_dedup_passed.push(node);
        } else {
            println!("  ✗ KILLED (batch title dup) \"{}\"", node.meta().unwrap().title);
            title_dedup_killed.push(node.meta().unwrap().title.clone());
        }
    }

    println!("\n  Title dedup result: {} passed, {} killed\n", title_dedup_passed.len(), title_dedup_killed.len());

    // ================================================================
    // STAGE 6: Global Title+Type Dedup (against Neo4j)
    // ================================================================
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  STAGE 6: Global Title+Type Dedup (vs graph)               ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let title_type_pairs: Vec<(String, NodeType)> = title_dedup_passed
        .iter()
        .map(|n| (n.meta().unwrap().title.trim().to_lowercase(), n.node_type()))
        .collect();

    let global_matches = writer
        .find_by_titles_and_types(&title_type_pairs)
        .await
        .unwrap_or_default();

    let mut global_dedup_passed = Vec::new();
    let mut global_dedup_killed = Vec::new();

    for node in title_dedup_passed {
        let key = (node.meta().unwrap().title.trim().to_lowercase(), node.node_type());
        if let Some((existing_id, existing_url)) = global_matches.get(&key) {
            if existing_url.as_str() == SUBREDDIT_URL {
                println!("  ✗ KILLED (same-source title match in graph, id={}) \"{}\"", existing_id, node.meta().unwrap().title);
            } else {
                println!("  ✗ CORROBORATE (cross-source title match, id={}, from \"{}\") \"{}\"",
                    existing_id, trunc(existing_url, 50), node.meta().unwrap().title);
            }
            global_dedup_killed.push(node.meta().unwrap().title.clone());
        } else {
            println!("  ✓ PASS (no global title match) \"{}\"", node.meta().unwrap().title);
            global_dedup_passed.push(node);
        }
    }

    println!("\n  Global dedup result: {} passed, {} killed/corroborated\n", global_dedup_passed.len(), global_dedup_killed.len());

    // ================================================================
    // STAGE 7: Embedding Dedup (against Neo4j vector index)
    // ================================================================
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  STAGE 7: Embedding Dedup (vector similarity vs graph)     ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let embedder = Embedder::new(&voyage_key);

    // Build embed texts exactly as scout.rs does
    let content_snippet = if all_combined_text.len() > 500 {
        &all_combined_text[..500]
    } else {
        &all_combined_text
    };

    let embed_texts: Vec<String> = global_dedup_passed
        .iter()
        .map(|n| format!("{} {}", n.meta().unwrap().title, content_snippet))
        .collect();

    if embed_texts.is_empty() {
        println!("  No signals left to embed.\n");
    } else {
        println!("  Embedding {} signals...\n", embed_texts.len());

        match embedder.embed_batch(embed_texts).await {
            Ok(embeddings) => {
                let mut embed_passed = Vec::new();
                let mut embed_killed = Vec::new();

                // Also check pairwise similarity between new signals
                println!("  --- Pairwise similarity between NEW signals ---\n");
                for i in 0..embeddings.len() {
                    for j in (i + 1)..embeddings.len() {
                        let sim = cosine_sim(&embeddings[i], &embeddings[j]);
                        let title_i = &global_dedup_passed[i].meta().unwrap().title;
                        let title_j = &global_dedup_passed[j].meta().unwrap().title;
                        if sim >= 0.75 {
                            let tag = if sim >= 0.92 {
                                "WOULD CORROBORATE"
                            } else if sim >= 0.85 {
                                "WOULD DEDUP"
                            } else {
                                "similar but passes"
                            };
                            println!("    sim={:.3} [{}]", sim, tag);
                            println!("      \"{}\"", trunc(title_i, 70));
                            println!("      \"{}\"", trunc(title_j, 70));
                            println!();
                        }
                    }
                }

                println!("  --- Checking each signal against graph index (threshold 0.85) ---\n");

                for (node, embedding) in global_dedup_passed.into_iter().zip(embeddings.into_iter()) {
                    let node_type = node.node_type();
                    let title = node.meta().unwrap().title.clone();

                    match writer.find_duplicate(&embedding, node_type, 0.85).await {
                        Ok(Some(dup)) => {
                            let is_same = dup.source_url.contains("reddit.com/r/Minneapolis");
                            if is_same {
                                println!("  ✗ KILLED (same-source embed dup, sim={:.3}, id={}) \"{}\"",
                                    dup.similarity, dup.id, title);
                                println!("    matched: id={} from {}", dup.id, trunc(&dup.source_url, 50));
                            } else if dup.similarity >= 0.92 {
                                println!("  ✗ CORROBORATE (cross-source, sim={:.3}, id={}) \"{}\"",
                                    dup.similarity, dup.id, title);
                                println!("    matched: id={} from {}", dup.id, trunc(&dup.source_url, 50));
                            } else {
                                println!("  ~ NEAR-MISS (sim={:.3}, below 0.92 cross-source threshold) \"{}\"",
                                    dup.similarity, title);
                                println!("    near: id={} from {}", dup.id, trunc(&dup.source_url, 50));
                                embed_passed.push((title.clone(), node_type));
                            }
                            embed_killed.push(title.clone());
                        }
                        Ok(None) => {
                            println!("  ✓ PASS (no embedding match in graph) \"{}\"", title);
                            embed_passed.push((title, node_type));
                        }
                        Err(e) => {
                            println!("  ? ERROR checking embedding: {} — \"{}\"", e, title);
                            embed_passed.push((title, node_type));
                        }
                    }
                }

                println!("\n  Embedding dedup result: {} passed, {} killed/corroborated\n",
                    embed_passed.len(), embed_killed.len());

                // ================================================================
                // SUMMARY
                // ================================================================
                println!("╔══════════════════════════════════════════════════════════════╗");
                println!("║  PIPELINE SUMMARY                                          ║");
                println!("╚══════════════════════════════════════════════════════════════╝\n");
                println!("  Apify posts returned:      {}", posts.len());
                println!("  LLM signals extracted:     {}", total_extracted);
                println!("  After quality scoring:     {} (all pass, just sets confidence)", total_extracted);
                println!("  After geo-filter:          {} (killed: {})", total_extracted - geo_killed.len(), geo_killed.len());
                for t in &geo_killed {
                    println!("    killed: \"{}\"", trunc(t, 70));
                }
                println!("  After batch title dedup:   {} (killed: {})", total_extracted - geo_killed.len() - title_dedup_killed.len(), title_dedup_killed.len());
                for t in &title_dedup_killed {
                    println!("    killed: \"{}\"", trunc(t, 70));
                }
                let global_dedup_count = embed_passed.len() + embed_killed.len();
                println!("  After global title dedup:  {} (killed: {})", global_dedup_count, global_dedup_killed.len());
                for t in &global_dedup_killed {
                    println!("    killed: \"{}\"", trunc(t, 70));
                }
                println!("  After embedding dedup:     {} (killed: {})", embed_passed.len(), embed_killed.len());
                for t in &embed_killed {
                    println!("    killed: \"{}\"", trunc(t, 70));
                }
                println!();
                println!("  WOULD BE STORED: {} signals", embed_passed.len());
                for (title, node_type) in &embed_passed {
                    println!("    [{:?}] \"{}\"", node_type, title);
                }
                println!();
            }
            Err(e) => println!("  Embedding FAILED: {}\n", e),
        }
    }

    Ok(())
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f64 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { return 0.0; }
    (dot / (norm_a * norm_b)) as f64
}

fn trunc(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}...", &s[..max]) }
}

fn node_meta_mut(node: &mut Node) -> Option<&mut rootsignal_common::NodeMeta> {
    match node {
        Node::Event(n) => Some(&mut n.meta),
        Node::Give(n) => Some(&mut n.meta),
        Node::Ask(n) => Some(&mut n.meta),
        Node::Notice(n) => Some(&mut n.meta),
        Node::Tension(n) => Some(&mut n.meta),
        Node::Evidence(_) => None,
    }
}

fn dotenv_load() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join(".env");
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some((key, value)) = line.split_once('=') {
                if std::env::var(key.trim()).is_err() {
                    std::env::set_var(key.trim(), value.trim());
                }
            }
        }
    }
}
