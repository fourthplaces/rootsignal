//! Social media scraping: fetch posts, extract signals via LLM.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use uuid::Uuid;

use seesaw_core::Logger;

use rootsignal_common::{
    scraping_strategy, ActorContext, ChannelWeights, Node, ScrapingStrategy, SocialPlatform,
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
            channel_weights: ChannelWeights,
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
                    channel_weights: source.channel_weights.clone(),
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
            let fetcher = fetcher.clone();
            let extractor = extractor.clone();
            let identifier = account.identifier.clone();
            let logger = logger.clone();
            let cw = account.channel_weights.clone();

            futures.push(Box::pin(async move {
                let fetch_feed = cw.feed > 0.0;
                let fetch_media = cw.media > 0.0;

                if !fetch_feed && !fetch_media {
                    logger.info(format!("{source_url}: all channels off, skipping"));
                    return None;
                }

                // --- Feed channel: posts ---
                let mut posts = Vec::new();
                if fetch_feed {
                    match fetcher.posts(&identifier, 20).await {
                        Ok(p) => {
                            logger.info(format!("{source_url}: fetched {} posts", p.len()));
                            posts = p;
                        }
                        Err(e) => {
                            logger.warn(format!("{source_url}: post fetch failed — {e}"));
                        }
                    }
                }

                // --- Media channel: stories + short videos ---
                let mut media_texts: Vec<String> = Vec::new();
                let mut media_permalink_map: HashMap<String, String> = HashMap::new();
                if fetch_media {
                    let stories = fetcher.stories(&identifier).await.unwrap_or_default();
                    let videos = fetcher.short_videos(&identifier, 10).await.unwrap_or_default();
                    if !stories.is_empty() || !videos.is_empty() {
                        logger.info(format!(
                            "{source_url}: fetched {} stories, {} videos",
                            stories.len(), videos.len(),
                        ));
                    }
                    for (i, story) in stories.iter().enumerate() {
                        if let Some(ref url) = story.permalink {
                            media_permalink_map.insert(format!("story_{}", i + 1), url.clone());
                        }
                        media_texts.push(super::shared::format_story(i, story));
                    }
                    for (i, video) in videos.iter().enumerate() {
                        if let Some(ref url) = video.permalink {
                            media_permalink_map.insert(format!("video_{}", i + 1), url.clone());
                        }
                        media_texts.push(super::shared::format_short_video(i, video));
                    }
                }

                let post_count = posts.len();
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
                        let permalink_map: HashMap<String, String> = batch
                            .iter()
                            .enumerate()
                            .filter_map(|(i, p)| {
                                p.permalink.as_ref().map(|url| (format!("post_{}", i + 1), url.clone()))
                            })
                            .collect();

                        let mut combined_text: String = batch
                            .iter()
                            .enumerate()
                            .map(|(i, p)| super::shared::format_post(i, p))
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        if combined_text.is_empty() {
                            continue;
                        }
                        if let Some(ref prefix) = actor_prefix {
                            combined_text = format!("{prefix}{combined_text}");
                        }
                        combined_all.push_str(&combined_text);
                        match extractor.extract(&combined_text, &source_url).await {
                            Ok(result) => {
                                if !result.rejected.is_empty() {
                                    logger.info(format!(
                                        "{source_url}: reddit batch — {} signals, {} rejected",
                                        result.nodes.len(), result.rejected.len(),
                                    ));
                                }
                                let mut nodes = result.nodes;
                                super::shared::resolve_source_ids(&mut nodes, &result.source_ids, &permalink_map);
                                all_nodes.extend(nodes);
                                all_resource_tags.extend(result.resource_tags);
                                all_signal_tags.extend(result.signal_tags);
                                all_author_actors.extend(result.author_actors);
                            }
                            Err(e) => {
                                logger.warn(format!("{source_url}: reddit batch extraction failed — {e}"));
                            }
                        }
                    }
                    if all_nodes.is_empty() && post_count > 0 {
                        logger.warn(format!("{source_url}: LLM returned 0 signals from {post_count} posts"));
                    }
                    if all_nodes.is_empty() && media_texts.is_empty() {
                        return None;
                    }
                    logger.info(format!("{source_url}: {post_count} posts → {} signals", all_nodes.len()));
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
                    // Instagram/Facebook/Twitter/TikTok: combine all content then extract
                    let mut permalink_map: HashMap<String, String> = posts
                        .iter()
                        .enumerate()
                        .filter_map(|(i, p)| {
                            p.permalink.as_ref().map(|url| (format!("post_{}", i + 1), url.clone()))
                        })
                        .collect();
                    permalink_map.extend(media_permalink_map);

                    let mut content_parts: Vec<String> = posts
                        .iter()
                        .enumerate()
                        .map(|(i, p)| super::shared::format_post(i, p))
                        .collect();
                    content_parts.extend(media_texts);

                    let mut combined_text = content_parts.join("\n\n");
                    if combined_text.is_empty() {
                        return None;
                    }
                    if let Some(ref prefix) = actor_prefix {
                        combined_text = format!("{prefix}{combined_text}");
                    }
                    let content_count = content_parts.len();
                    let mut result = match extractor.extract(&combined_text, &source_url).await {
                        Ok(r) => r,
                        Err(e) => {
                            logger.warn(format!("{source_url}: extraction failed — {e}"));
                            return None;
                        }
                    };
                    // Retry once when LLM returns nothing from substantial content
                    if result.nodes.is_empty() && result.raw_signal_count == 0 && content_count >= 5 {
                        logger.info(format!("{source_url}: 0 signals from {content_count} items, retrying"));
                        result = match extractor.extract(&combined_text, &source_url).await {
                            Ok(r) => r,
                            Err(e) => {
                                logger.warn(format!("{source_url}: retry extraction failed — {e}"));
                                return None;
                            }
                        };
                    }
                    super::shared::resolve_source_ids(&mut result.nodes, &result.source_ids, &permalink_map);
                    if result.nodes.is_empty() {
                        if result.raw_signal_count == 0 {
                            logger.warn(format!("{source_url}: LLM returned 0 signals from {content_count} items"));
                        } else {
                            logger.info(format!(
                                "{source_url}: LLM returned {} signals but all rejected ({} not firsthand) from {content_count} items",
                                result.raw_signal_count,
                                result.rejected.len(),
                            ));
                        }
                    } else {
                        let rejected = result.rejected.len();
                        if rejected > 0 {
                            logger.info(format!(
                                "{source_url}: {content_count} items → {} signals ({rejected} rejected)",
                                result.nodes.len(),
                            ));
                        } else {
                            logger.info(format!("{source_url}: {content_count} items → {} signals", result.nodes.len()));
                        }
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
                let before_dedup = nodes.len();
                let nodes = batch_title_dedup(nodes);
                if nodes.len() < before_dedup {
                    logger.info(format!(
                        "{source_url}: batch title dedup dropped {} of {before_dedup}",
                        before_dedup - nodes.len(),
                    ));
                }

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

