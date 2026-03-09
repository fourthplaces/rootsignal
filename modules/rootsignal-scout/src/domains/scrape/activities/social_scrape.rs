//! Social media scraping: fetch posts, extract signals via LLM.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Duration, Utc};
use futures::stream::{self, StreamExt};
use uuid::Uuid;

use seesaw_core::Logger;

use rootsignal_common::{
    scraping_strategy, ActorContext, ChannelWeights, Node, ScrapingStrategy, SocialPlatform,
    SourceNode,
};

use crate::core::aggregate::ExtractedBatch;
use crate::core::engine::ScoutEngineDeps;
use crate::core::extractor::{ResourceTag, SignalExtractor};
use crate::domains::enrichment::activities::link_promoter::{self, CollectedLink};

use super::types::{batch_title_dedup, score_and_filter, ScrapeOutput, UrlExtraction};

const EXTRACTION_BATCH_SIZE: usize = 5;

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
            has_unenriched_media: bool,
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
                let mut media_items: Vec<ContentItem> = Vec::new();
                let mut has_unenriched_media = false;
                if fetch_media {
                    let stories = fetcher.stories(&identifier).await.unwrap_or_default();
                    let videos = fetcher.short_videos(&identifier, 10).await.unwrap_or_default();
                    if !stories.is_empty() || !videos.is_empty() {
                        logger.info(format!(
                            "{source_url}: fetched {} stories, {} videos",
                            stories.len(), videos.len(),
                        ));
                    }
                    has_unenriched_media = stories.iter().any(|s| {
                        s.attachments.iter().any(|a| a.text.is_none())
                    }) || videos.iter().any(|v| {
                        v.attachments.iter().any(|a| a.text.is_none())
                    });

                    for (i, story) in stories.iter().enumerate() {
                        media_items.push(ContentItem {
                            text: super::shared::format_story(i, story),
                            key: format!("story_{}", i + 1),
                            permalink: story.permalink.clone(),
                        });
                    }
                    for (i, video) in videos.iter().enumerate() {
                        media_items.push(ContentItem {
                            text: super::shared::format_short_video(i, video),
                            key: format!("video_{}", i + 1),
                            permalink: video.permalink.clone(),
                        });
                    }
                }

                let post_count = posts.len();
                let newest_published_at = posts.iter().filter_map(|p| p.published_at).max();
                let source_mentions: Vec<String> = posts
                    .iter()
                    .flat_map(|p| p.mentions.iter().cloned())
                    .collect();

                // Build content items: posts first, then media
                let mut items: Vec<ContentItem> = posts
                    .iter()
                    .enumerate()
                    .map(|(i, p)| ContentItem {
                        text: super::shared::format_post(i, p),
                        key: format!("post_{}", i + 1),
                        permalink: p.permalink.clone(),
                    })
                    .collect();
                items.extend(media_items);

                if items.is_empty() {
                    return None;
                }

                let content_count = items.len();
                let posts_with_text = posts.iter().filter(|p| p.text.as_ref().is_some_and(|t| !t.is_empty())).count();
                logger.info(format!(
                    "{source_url}: {content_count} items ({posts_with_text}/{post_count} posts have text)",
                ));

                let result = extract_content_batches(
                    items,
                    actor_prefix.as_deref(),
                    extractor.as_ref(),
                    &source_url,
                    &logger,
                ).await;

                if result.nodes.is_empty() {
                    if post_count > 0 {
                        let preview: String = result.combined_text.chars().take(500).collect();
                        logger.warn(format!(
                            "{source_url}: 0 signals from {content_count} items. Preview:\n{preview}",
                        ));
                    }
                    return None;
                }

                logger.info(format!(
                    "{source_url}: {content_count} items → {} signals",
                    result.nodes.len(),
                ));

                Some(SocialFetchResult {
                    canonical_key,
                    source_url,
                    platform,
                    combined_text: result.combined_text,
                    nodes: result.nodes,
                    resource_tags: result.resource_tags,
                    signal_tags: result.signal_tags,
                    author_actors: result.author_actors,
                    post_count,
                    mentions: source_mentions,
                    newest_published_at,
                    has_unenriched_media,
                })
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
                has_unenriched_media,
            } = result;

            // Apply social published_at as fallback published_at when LLM didn't extract one
            if let Some(pub_at) = newest_published_at {
                super::shared::apply_published_at_fallback(&mut nodes, pub_at);
            }

            for handle in mentions.into_iter().take(promotion_config.max_per_source) {
                if let Some(mention_url) = link_promoter::platform_url(&result_platform, &handle) {
                    output.collected_links.push(CollectedLink {
                        url: mention_url,
                        discovered_on: source_url.clone(),
                    });
                }
            }

            if has_unenriched_media {
                if let Some(&source_id) = ck_to_source_id.get(&canonical_key) {
                    logger.info(format!(
                        "{source_url}: media attachments pending enrichment, scheduling re-scrape",
                    ));
                    output.events.push(
                        crate::domains::scheduling::events::SchedulingEvent::ScrapeScheduled {
                            scope: crate::domains::scheduling::events::ScheduledScope::Sources {
                                source_ids: vec![source_id],
                            },
                            run_after: Utc::now() + Duration::hours(1),
                            reason: "media enrichment pending (OCR/transcription)".into(),
                        },
                    );
                }
            }

            output.expansion_queries.extend(super::shared::collect_implied_queries(&nodes));
            output.stats_delta.social_media_posts += post_count as u32;
            let source_id = ck_to_source_id.get(&canonical_key).copied();

            let ck_for_fallback = url_to_canonical_key
                .get(&source_url)
                .cloned()
                .unwrap_or_else(|| source_url.clone());
            let actor_ctx = actor_contexts.get(&ck_for_fallback);
            let nodes = score_and_filter(nodes, actor_ctx);

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

/// A single content item (post, story, or video) ready for LLM extraction.
struct ContentItem {
    /// Formatted text (e.g. "--- Post 3 ---\n...")
    text: String,
    /// Permalink key (e.g. "post_3", "story_1") for source ID resolution.
    key: String,
    /// Actual URL for this content item.
    permalink: Option<String>,
}

/// Merged extraction results across all batches for one source.
struct BatchedExtractionResult {
    nodes: Vec<Node>,
    resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
    signal_tags: Vec<(Uuid, Vec<String>)>,
    author_actors: HashMap<Uuid, String>,
    combined_text: String,
}

/// Chunk content items into batches, extract each via LLM, merge results.
///
/// Each batch gets its own permalink map for source ID resolution.
/// Failed batches are logged and skipped — one bad batch doesn't kill the source.
async fn extract_content_batches(
    items: Vec<ContentItem>,
    actor_prefix: Option<&str>,
    extractor: &dyn SignalExtractor,
    source_url: &str,
    logger: &Logger,
) -> BatchedExtractionResult {
    let mut all_nodes = Vec::new();
    let mut all_resource_tags = Vec::new();
    let mut all_signal_tags = Vec::new();
    let mut all_author_actors: HashMap<Uuid, String> = HashMap::new();
    let mut combined_text = String::new();

    for chunk in items.chunks(EXTRACTION_BATCH_SIZE) {
        // Per-batch permalink map: batch-relative indices avoid cross-batch collision
        let permalink_map: HashMap<String, String> = chunk
            .iter()
            .filter_map(|item| {
                item.permalink.as_ref().map(|url| (item.key.clone(), url.clone()))
            })
            .collect();

        let mut batch_text: String = chunk
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        if batch_text.is_empty() {
            continue;
        }
        if let Some(prefix) = actor_prefix {
            batch_text = format!("{prefix}{batch_text}");
        }
        combined_text.push_str(&batch_text);

        let batch_size = chunk.len();
        logger.info(format!(
            "{source_url}: extracting batch of {batch_size} items ({} bytes)",
            batch_text.len(),
        ));

        match extractor.extract(&batch_text, source_url).await {
            Ok(result) => {
                if !result.rejected.is_empty() {
                    logger.info(format!(
                        "{source_url}: batch — {} signals, {} rejected",
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
                logger.warn(format!("{source_url}: batch extraction failed — {e}"));
            }
        }
    }

    BatchedExtractionResult {
        nodes: all_nodes,
        resource_tags: all_resource_tags,
        signal_tags: all_signal_tags,
        author_actors: all_author_actors,
        combined_text,
    }
}

