//! Social media scraping: fetch posts, extract signals via LLM.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use uuid::Uuid;

use seesaw_core::Logger;

use rootsignal_common::{
    scraping_strategy, ActorContext, Node, ScrapingStrategy, SocialPlatform,
    SourceNode,
};

use crate::core::aggregate::ExtractedBatch;
use crate::core::engine::ScoutEngineDeps;
use crate::core::extractor::ResourceTag;
use crate::domains::enrichment::activities::link_promoter::{self, CollectedLink};

use super::types::{batch_title_dedup, score_and_filter, ScrapeOutput, UrlExtraction};

/// Scrape social media accounts, feed posts through LLM extraction.
/// Returns accumulated `ScrapeOutput` with events and state updates.
pub(crate) async fn scrape_social_sources(
    deps: &ScoutEngineDeps,
    social_sources: &[&SourceNode],
    url_to_canonical_key: &HashMap<String, String>,
    actor_contexts: &HashMap<String, ActorContext>,
    logger: &Logger,
) -> ScrapeOutput {
        let mut output = ScrapeOutput::new();

        struct SocialFetchResult {
            canonical_key: String,
            source_url: String,
            platform: SocialPlatform,
            combined_text: String,
            nodes: Vec<Node>,
            resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
            signal_tags: Vec<(Uuid, Vec<String>)>,
            author_actors: HashMap<Uuid, String>,
            post_count: usize,
            mentions: Vec<String>,
            newest_published_at: Option<DateTime<Utc>>,
        }

        // Build uniform list of (canonical_key, source_url, platform, fetch_identifier) from SourceNodes
        struct SocialEntry {
            platform: SocialPlatform,
            identifier: String,
        }
        let mut accounts: Vec<(String, String, SocialEntry)> = Vec::new();

        for source in social_sources {
            let common_platform = match scraping_strategy(source.value()) {
                ScrapingStrategy::Social(p) => p,
                _ => continue,
            };
            let (platform, identifier) = match common_platform {
                SocialPlatform::Instagram => (
                    SocialPlatform::Instagram,
                    source
                        .url
                        .as_deref()
                        .unwrap_or(&source.canonical_value)
                        .to_string(),
                ),
                SocialPlatform::Facebook => {
                    let url = source
                        .url
                        .as_deref()
                        .filter(|u| !u.is_empty())
                        .unwrap_or(&source.canonical_value);
                    (SocialPlatform::Facebook, url.to_string())
                }
                SocialPlatform::Reddit => {
                    let url = source
                        .url
                        .as_deref()
                        .filter(|u| !u.is_empty())
                        .unwrap_or(&source.canonical_value);
                    let identifier = if !url.starts_with("http") {
                        let name = url.trim_start_matches("r/");
                        format!("https://www.reddit.com/r/{}/", name)
                    } else {
                        url.to_string()
                    };
                    (SocialPlatform::Reddit, identifier)
                }
                SocialPlatform::Twitter => (
                    SocialPlatform::Twitter,
                    source
                        .url
                        .as_deref()
                        .unwrap_or(&source.canonical_value)
                        .to_string(),
                ),
                SocialPlatform::TikTok => (
                    SocialPlatform::TikTok,
                    source
                        .url
                        .as_deref()
                        .unwrap_or(&source.canonical_value)
                        .to_string(),
                ),
                SocialPlatform::Bluesky => continue,
            };
            let source_url = source
                .url
                .as_deref()
                .filter(|u| !u.is_empty())
                .unwrap_or(&source.canonical_value)
                .to_string();
            accounts.push((
                source.canonical_key.clone(),
                source_url,
                SocialEntry {
                    platform,
                    identifier,
                },
            ));
        }

        let source_names: Vec<String> = accounts
            .iter()
            .map(|(ck, _, a)| format!("{} ({})", ck, match a.platform {
                SocialPlatform::Instagram => "ig",
                SocialPlatform::Facebook => "fb",
                SocialPlatform::Reddit => "reddit",
                SocialPlatform::Twitter => "twitter",
                SocialPlatform::TikTok => "tiktok",
                SocialPlatform::Bluesky => "bsky",
            }))
            .collect();
        logger.info(format!("Scraping {} social sources: {}", accounts.len(), source_names.join(", ")));

        let actor_prefixes: HashMap<String, String> = accounts
            .iter()
            .filter_map(|(ck, _, _)| {
                actor_contexts.get(ck).map(|ac| {
                    let mut prefix = format!(
                        "ACTOR CONTEXT: This content is from {}", ac.actor_name
                    );
                    if let Some(ref bio) = ac.bio {
                        prefix.push_str(&format!(", {}", bio));
                    }
                    if let Some(ref loc) = ac.location_name {
                        prefix.push_str(&format!(", located in {}", loc));
                    }
                    prefix.push_str(". Use this location as fallback if the post doesn't mention a specific place.\n\n");
                    (ck.clone(), prefix)
                })
            })
            .collect();

        // First-hand filter prefix for non-entity social sources
        let firsthand_filter = "FIRST-HAND FILTER (applies to this content):\n\
            This content comes from platform search results, which are flooded with \
            political commentary from people not directly involved. Apply strict filtering:\n\n\
            For each potential signal, assess: Is this person describing something happening \
            to them, their family, their community, or their neighborhood? Or are they \
            asking for help? If yes, mark is_firsthand: true. If this is political commentary \
            from someone not personally affected — regardless of viewpoint — mark \
            is_firsthand: false.\n\n\
            Signal: \"My family was taken.\" → is_firsthand: true\n\
            Signal: \"There were raids on 5th street today.\" → is_firsthand: true\n\
            Signal: \"We need legal observers.\" → is_firsthand: true\n\
            Noise: \"ICE is doing great work.\" → is_firsthand: false\n\
            Noise: \"The housing crisis is a failure of capitalism.\" → is_firsthand: false\n\n\
            Only extract signals where is_firsthand is true. Reject the rest.\n\n";

        // Collect all futures into a single Vec<Pin<Box<...>>> so types unify
        let mut futures: Vec<Pin<Box<dyn Future<Output = Option<SocialFetchResult>> + Send>>> = Vec::new();

        let fetcher = deps.fetcher.as_ref().expect("fetcher required").clone();
        let extractor = deps.extractor.as_ref().expect("extractor required").clone();
        for (canonical_key, source_url, account) in &accounts {
            let canonical_key = canonical_key.clone();
            let source_url = source_url.clone();
            let platform = account.platform;
            let is_reddit = matches!(platform, SocialPlatform::Reddit);
            let actor_prefix = actor_prefixes.get(&canonical_key).cloned();
            let firsthand_prefix = if actor_prefix.is_none() {
                Some(firsthand_filter.to_string())
            } else {
                None
            };
            let fetcher = fetcher.clone();
            let extractor = extractor.clone();
            let identifier = account.identifier.clone();
            let logger = logger.clone();

            futures.push(Box::pin(async move {
                let posts = match fetcher.posts(&identifier, 20).await {
                    Ok(posts) => posts,
                    Err(e) => {
                        logger.warn(format!("{source_url}: fetch failed — {e}"));
                        return None;
                    }
                };
                let post_count = posts.len();
                logger.info(format!("{source_url}: fetched {post_count} posts"));

                let newest_published_at = posts.iter().filter_map(|p| p.published_at).max();

                let source_mentions: Vec<String> = posts
                    .iter()
                    .flat_map(|p| p.mentions.iter().cloned())
                    .collect();

                if is_reddit {
                    // Reddit: batch posts 10 at a time for extraction
                    let batches: Vec<_> = posts.chunks(10).collect();
                    let mut all_nodes = Vec::new();
                    let mut all_resource_tags = Vec::new();
                    let mut all_signal_tags = Vec::new();
                    let mut all_author_actors: HashMap<Uuid, String> = HashMap::new();
                    let mut combined_all = String::new();
                    for batch in batches {
                        let mut combined_text: String = batch
                            .iter()
                            .enumerate()
                            .map(|(i, p)| super::shared::format_post(i, p))
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        if combined_text.is_empty() {
                            continue;
                        }
                        // Prepend entity context for known actor sources,
                        // or first-hand filter for non-entity sources
                        if let Some(ref prefix) = actor_prefix {
                            combined_text = format!("{prefix}{combined_text}");
                        } else if let Some(ref prefix) = firsthand_prefix {
                            combined_text = format!("{prefix}{combined_text}");
                        }
                        combined_all.push_str(&combined_text);
                        match extractor.extract(&combined_text, &source_url).await {
                            Ok(result) => {
                                all_nodes.extend(result.nodes);
                                all_resource_tags.extend(result.resource_tags);
                                all_signal_tags.extend(result.signal_tags);
                                all_author_actors.extend(result.author_actors);
                            }
                            Err(e) => {
                                logger.warn(format!("{source_url}: reddit batch extraction failed — {e}"));
                            }
                        }
                    }
                    if all_nodes.is_empty() {
                        logger.info(format!("{source_url}: no signals extracted from {post_count} posts"));
                        return None;
                    }
                    logger.info(format!("{source_url}: extracted {} signals from {post_count} posts", all_nodes.len()));
                    Some(SocialFetchResult {
                        canonical_key,
                        source_url,
                        platform,
                        combined_text: combined_all,
                        nodes: all_nodes,
                        resource_tags: all_resource_tags,
                        signal_tags: all_signal_tags,
                        author_actors: all_author_actors,
                        post_count,
                        mentions: source_mentions,
                        newest_published_at,
                    })
                } else {
                    // Instagram/Facebook/Twitter/TikTok: combine all posts then extract
                    let mut combined_text: String = posts
                        .iter()
                        .enumerate()
                        .map(|(i, p)| super::shared::format_post(i, p))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    if combined_text.is_empty() {
                        return None;
                    }
                    // Prepend entity context for known actor sources,
                    // or first-hand filter for non-entity sources
                    if let Some(ref prefix) = actor_prefix {
                        combined_text = format!("{prefix}{combined_text}");
                    } else if let Some(ref prefix) = firsthand_prefix {
                        combined_text = format!("{prefix}{combined_text}");
                    }
                    let result = match extractor.extract(&combined_text, &source_url).await {
                        Ok(r) => r,
                        Err(e) => {
                            logger.warn(format!("{source_url}: extraction failed — {e}"));
                            return None;
                        }
                    };
                    if result.nodes.is_empty() {
                        logger.info(format!("{source_url}: no signals extracted from {post_count} posts"));
                    } else {
                        logger.info(format!("{source_url}: extracted {} signals from {post_count} posts", result.nodes.len()));
                    }
                    Some(SocialFetchResult {
                        canonical_key,
                        source_url,
                        platform,
                        combined_text,
                        nodes: result.nodes,
                        resource_tags: result.resource_tags,
                        signal_tags: result.signal_tags,
                        author_actors: result.author_actors.into_iter().collect(),
                        post_count,
                        mentions: source_mentions,
                        newest_published_at,
                    })
                }
            }));
        }

        let results: Vec<_> = stream::iter(futures).buffer_unordered(10).collect().await;

        let promotion_config = link_promoter::PromotionConfig::default();
        let ck_to_source_id: HashMap<String, Uuid> = social_sources
            .iter()
            .map(|s| (s.canonical_key.clone(), s.id))
            .collect();
        for result in results.into_iter().flatten() {
            let SocialFetchResult {
                canonical_key,
                source_url,
                platform: result_platform,
                combined_text,
                mut nodes,
                resource_tags,
                signal_tags,
                author_actors,
                post_count,
                mentions,
                newest_published_at,
            } = result;

            // Apply social published_at as fallback published_at when LLM didn't extract one
            if let Some(pub_at) = newest_published_at {
                super::shared::apply_published_at_fallback(&mut nodes, pub_at);
            }

            for handle in mentions.into_iter().take(promotion_config.max_per_source) {
                let mention_url = link_promoter::platform_url(&result_platform, &handle);
                output.collected_links.push(CollectedLink {
                    url: mention_url,
                    discovered_on: source_url.clone(),
                });
            }

            output.expansion_queries.extend(super::shared::collect_implied_queries(&nodes));
            output.stats_delta.social_media_posts += post_count as u32;
            let source_id = ck_to_source_id.get(&canonical_key).copied();

            let ck_for_fallback = url_to_canonical_key
                .get(&source_url)
                .cloned()
                .unwrap_or_else(|| source_url.clone());
            let actor_ctx = actor_contexts.get(&ck_for_fallback);
            let nodes = score_and_filter(nodes, &source_url, actor_ctx);

            if !nodes.is_empty() {
                let nodes = batch_title_dedup(nodes);

                let ck = url_to_canonical_key
                    .get(&source_url)
                    .cloned()
                    .unwrap_or_else(|| source_url.clone());

                let batch = ExtractedBatch {
                    content: combined_text,
                    nodes,
                    resource_tags: resource_tags.into_iter().collect(),
                    signal_tags: signal_tags.into_iter().collect(),
                    author_actors,
                    source_id,
                };

                *output.source_signal_counts.entry(canonical_key).or_default() += batch.nodes.len() as u32;
                output.extracted_batches.push(UrlExtraction {
                    url: source_url,
                    canonical_key: ck,
                    batch,
                });
            }
        }
        logger.info(format!(
            "Social scrape complete: {} posts fetched, {} batches with signals",
            output.stats_delta.social_media_posts,
            output.extracted_batches.len(),
        ));
        output
    }

