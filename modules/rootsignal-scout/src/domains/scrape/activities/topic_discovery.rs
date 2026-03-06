//! Topic discovery: search social platforms by topic, discover new accounts.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    canonical_value, is_web_query, ActorContext, DiscoveryMethod, SocialPlatform, SourceNode,
    SourceRole,
};

use crate::core::aggregate::ExtractedBatch;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::enrichment::activities::link_promoter::{self, CollectedLink};

use super::types::{batch_title_dedup, score_and_filter, ScrapeOutput, UrlExtraction};

/// Discover new accounts by searching platform-agnostic topics (hashtags/keywords)
/// across Instagram, X/Twitter, TikTok, and GoFundMe.
pub(crate) async fn discover_from_topics(
    deps: &ScoutEngineDeps,
    topics: &[String],
    url_to_canonical_key: &HashMap<String, String>,
    actor_contexts: &HashMap<String, ActorContext>,
) -> ScrapeOutput {
        let mut output = ScrapeOutput::new();
        const MAX_SOCIAL_SEARCHES: usize = 10;
        const MAX_NEW_ACCOUNTS: usize = 10;
        const POSTS_PER_SEARCH: u32 = 30;
        const MAX_SITE_SEARCH_TOPICS: usize = 4;
        const SITE_SEARCH_RESULTS: usize = 5;

    let store = &deps.store;
    let fetcher = deps.fetcher.as_ref().expect("fetcher required");
    let extractor = deps.extractor.as_ref().expect("extractor required");

        if topics.is_empty() {
            return output;
        }

        info!(topics = ?topics, "Starting social topic discovery...");

        let known_urls: HashSet<String> = url_to_canonical_key.keys().cloned().collect();

        let existing_sources = store.get_active_sources().await.unwrap_or_default();
        let existing_canonical_values: HashSet<String> = existing_sources
            .iter()
            .map(|s| s.canonical_value.clone())
            .collect();

        let mut new_accounts = 0u32;
        let mut new_sources: Vec<SourceNode> = Vec::new();
        let topic_strs: Vec<&str> = topics
            .iter()
            .take(MAX_SOCIAL_SEARCHES)
            .map(|t| t.as_str())
            .collect();

        let platform_urls: &[(&str, &str)] = &[
            ("instagram", "https://www.instagram.com/topics"),
            ("x", "https://x.com/topics"),
            ("tiktok", "https://www.tiktok.com/topics"),
            ("reddit", "https://www.reddit.com/topics"),
        ];

        for &(platform_name, platform_url) in platform_urls {
            if new_accounts >= MAX_NEW_ACCOUNTS as u32 {
                break;
            }

            let discovered_posts = match fetcher
                .search_topics(platform_url, &topic_strs, POSTS_PER_SEARCH)
                .await
            {
                Ok(posts) => posts,
                Err(e) => {
                    warn!(platform = platform_name, error = %e, "Topic discovery failed for platform");
                    continue;
                }
            };

            if discovered_posts.is_empty() {
                info!(
                    platform = platform_name,
                    "No posts found from topic discovery"
                );
                continue;
            }

            output.stats_delta.discovery_posts_found += discovered_posts.len() as u32;

            let mut by_author: HashMap<String, Vec<&rootsignal_common::Post>> = HashMap::new();
            for post in &discovered_posts {
                if let Some(ref author) = post.author {
                    by_author.entry(author.clone()).or_default().push(post);
                }
            }

            info!(
                platform = platform_name,
                posts = discovered_posts.len(),
                unique_authors = by_author.len(),
                "Topic discovery posts grouped by author"
            );

            let platform_enum = match platform_name {
                "instagram" => Some(SocialPlatform::Instagram),
                "x" => Some(SocialPlatform::Twitter),
                "tiktok" => Some(SocialPlatform::TikTok),
                "reddit" => Some(SocialPlatform::Reddit),
                _ => None,
            };

            for (username, posts) in &by_author {
                if new_accounts >= MAX_NEW_ACCOUNTS as u32 {
                    info!("Discovery account budget exhausted");
                    break;
                }

                let source_url = match platform_name {
                    "instagram" => format!("https://www.instagram.com/{username}/"),
                    "x" => format!("https://x.com/{username}"),
                    "tiktok" => format!("https://www.tiktok.com/@{username}"),
                    "reddit" => format!("https://www.reddit.com/user/{username}/"),
                    _ => continue,
                };

                if existing_canonical_values.contains(&username.to_string()) {
                    continue;
                }

                let combined_text: String = posts
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.text.as_deref().is_some_and(|t| !t.is_empty()))
                    .map(|(i, p)| super::shared::format_post(i, p))
                    .collect::<Vec<_>>()
                    .join("\n\n");

                if combined_text.is_empty() {
                    continue;
                }

                let result = match extractor.extract(&combined_text, &source_url).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(username, platform = platform_name, error = %e, "Discovery extraction failed");
                        continue;
                    }
                };

                if result.nodes.is_empty() {
                    continue;
                }

                let author_actors: HashMap<Uuid, String> =
                    result.author_actors.into_iter().collect();

                let actor_ctx = actor_contexts.get(&source_url);
                let nodes = score_and_filter(result.nodes, &source_url, actor_ctx);
                let produced = if !nodes.is_empty() {
                    let nodes = batch_title_dedup(nodes);
                    let count = nodes.len() as u32;

                    let batch = ExtractedBatch {
                        content: combined_text,
                        nodes,
                        resource_tags: result.resource_tags.into_iter().collect(),
                        signal_tags: result.signal_tags.into_iter().collect(),
                        author_actors,
                        source_id: None,
                    };

                    output.extracted_batches.push(UrlExtraction {
                        url: source_url.clone(),
                        canonical_key: source_url.clone(),
                        batch,
                    });
                    count
                } else {
                    0
                };

                // Only follow mentions from authors whose posts produced signals
                if produced > 0 {
                    if let Some(ref sp) = platform_enum {
                        for post in posts {
                            for handle in post.mentions.iter().take(5) {
                                let mention_url = link_promoter::platform_url(sp, handle);
                                output.collected_links.push(CollectedLink {
                                    url: mention_url,
                                    discovered_on: source_url.clone(),
                                });
                            }
                        }
                    }
                }

                let cv = rootsignal_common::canonical_value(&source_url);
                let ck = canonical_value(&source_url);
                let gap_context = format!(
                    "Topic: {}",
                    topics.first().map(|t| t.as_str()).unwrap_or("unknown")
                );
                let source = SourceNode {
                    last_scraped: Some(Utc::now()),
                    last_produced_signal: if produced > 0 { Some(Utc::now()) } else { None },
                    signals_produced: produced,
                    ..SourceNode::new(
                        ck.clone(),
                        cv,
                        Some(source_url.clone()),
                        DiscoveryMethod::HashtagDiscovery,
                        0.3,
                        SourceRole::default(),
                        Some(gap_context),
                    )
                };

                *output.source_signal_counts.entry(ck).or_default() += produced;

                new_sources.push(source);
                new_accounts += 1;
                info!(
                    username,
                    platform = platform_name,
                    signals = produced,
                    "Discovered new account via topic search"
                );
            }
        }

        // Site-scoped search: find WebQuery sources with `site:` prefix,
        // search Serper for each topic, scrape + extract results.
        let site_sources: Vec<&SourceNode> = existing_sources
            .iter()
            .filter(|s| is_web_query(&s.canonical_value) && s.canonical_value.starts_with("site:"))
            .collect();

        for source in &site_sources {
            let site_prefix = &source.canonical_value; // e.g. "site:gofundme.com/f/ Minneapolis"
            for topic in topics.iter().take(MAX_SITE_SEARCH_TOPICS) {
                let query = format!("{} {}", site_prefix, topic);

                let search_results =
                    match fetcher.site_search(&query, SITE_SEARCH_RESULTS).await {
                        Ok(r) => r,
                        Err(e) => {
                            warn!(query, error = %e, "Site-scoped search failed");
                            continue;
                        }
                    };

                if search_results.results.is_empty() {
                    continue;
                }

                info!(
                    query,
                    count = search_results.results.len(),
                    "Site-scoped search results"
                );

                for result in &search_results.results {
                    if known_urls.contains(&result.url) {
                        continue;
                    }

                    let page = match fetcher.page(&result.url).await {
                        Ok(p) => p,
                        Err(e) => {
                            warn!(url = result.url.as_str(), error = %e, "Site-scoped scrape failed");
                            continue;
                        }
                    };
                    if page.markdown.is_empty() {
                        continue;
                    }
                    let content = page.markdown;

                    let extracted = match extractor.extract(&content, &result.url).await {
                        Ok(r) => r,
                        Err(e) => {
                            warn!(url = result.url, error = %e, "Site-scoped extraction failed");
                            continue;
                        }
                    };

                    if extracted.nodes.is_empty() {
                        continue;
                    }

                    let author_actors: HashMap<Uuid, String> =
                        extracted.author_actors.into_iter().collect();

                    let actor_ctx = actor_contexts.get(&result.url);
                    let nodes = score_and_filter(extracted.nodes, &result.url, actor_ctx);
                    if !nodes.is_empty() {
                        let nodes = batch_title_dedup(nodes);

                        let batch = ExtractedBatch {
                            content,
                            nodes,
                            resource_tags: extracted.resource_tags.into_iter().collect(),
                            signal_tags: extracted.signal_tags.into_iter().collect(),
                            author_actors,
                            source_id: None,
                        };

                        output.extracted_batches.push(UrlExtraction {
                            url: result.url.clone(),
                            canonical_key: result.url.clone(),
                            batch,
                        });
                    }
                }
            }
        }

        // Collect discovered sources as data (emitted at phase boundary)
        output.discovered_sources.extend(new_sources);

        output.stats_delta.discovery_accounts_found = new_accounts;
        info!(
            topics = topics.len(),
            new_accounts, "Social topic discovery complete"
        );
        output
    }
