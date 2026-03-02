//! Social media scraping: fetch posts, extract signals via LLM.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    scraping_strategy, ActorContext, Node, NodeType, Post, ScrapingStrategy, SocialPlatform,
    SourceNode,
};
use seesaw_core::Events;

use crate::core::extractor::ResourceTag;
use crate::domains::enrichment::activities::link_promoter::{self, CollectedLink};
use crate::infra::run_log::{EventKind, EventLogger, RunLogger};

use super::scraper::Scraper;
use super::signal_events::store_signals_events;
use super::types::ScrapeOutput;

impl Scraper {
    /// Scrape social media accounts, feed posts through LLM extraction.
    /// Returns accumulated `ScrapeOutput` with events and state updates.
    pub async fn scrape_social_sources(
        &self,
        social_sources: &[&SourceNode],
        url_to_canonical_key: &HashMap<String, String>,
        actor_contexts: &HashMap<String, ActorContext>,
        run_log: &RunLogger,
    ) -> ScrapeOutput {
        let mut output = ScrapeOutput::new();
        type SocialResult = Option<(
            String,
            String,
            SocialPlatform,
            String,
            Vec<Node>,
            Vec<(Uuid, Vec<ResourceTag>)>,
            Vec<(Uuid, Vec<String>)>,
            HashMap<Uuid, String>,
            usize,
            Vec<String>,
            Option<DateTime<Utc>>, // most recent published_at for published_at fallback
        )>; // (canonical_key, source_url, platform, combined_text, nodes, resource_tags, signal_tags, author_actors, post_count, mentions, newest_published_at)

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

        let ig_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::Instagram))
            .count();
        let fb_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::Facebook))
            .count();
        let reddit_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::Reddit))
            .count();
        let twitter_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::Twitter))
            .count();
        let tiktok_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::TikTok))
            .count();
        info!(
            ig = ig_count,
            fb = fb_count,
            reddit = reddit_count,
            twitter = twitter_count,
            tiktok = tiktok_count,
            "Scraping social media..."
        );

        // Build actor context prefixes for known actor sources
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
        let mut futures: Vec<Pin<Box<dyn Future<Output = SocialResult> + Send>>> = Vec::new();

        let fetcher = self.fetcher.clone();
        let extractor = self.extractor.clone();
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

            futures.push(Box::pin(async move {
                let posts = match fetcher.posts(&identifier, 20).await {
                    Ok(posts) => posts,
                    Err(e) => {
                        warn!(source_url, error = %e, "Social media scrape failed");
                        return None;
                    }
                };
                let post_count = posts.len();

                // Find the most recent published_at for published_at fallback
                let newest_published_at = posts.iter().filter_map(|p| p.published_at).max();

                // Collect @mentions from posts
                let source_mentions: Vec<String> = posts
                    .iter()
                    .flat_map(|p| p.mentions.iter().cloned())
                    .collect();

                // Format a post header including the specific post URL when available.
                let post_header = |i: usize, p: &Post| -> String {
                    let text = p.text.as_deref().unwrap_or("");
                    match &p.permalink {
                        Some(url) => format!("--- Post {} ({}) ---\n{}", i + 1, url, text),
                        None => format!("--- Post {} ---\n{}", i + 1, text),
                    }
                };

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
                            .map(|(i, p)| post_header(i, p))
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
                                warn!(source_url, error = %e, "Reddit extraction failed");
                            }
                        }
                    }
                    if all_nodes.is_empty() {
                        return None;
                    }
                    info!(source_url, posts = post_count, "Reddit scrape complete");
                    Some((
                        canonical_key,
                        source_url,
                        platform,
                        combined_all,
                        all_nodes,
                        all_resource_tags,
                        all_signal_tags,
                        all_author_actors,
                        post_count,
                        source_mentions,
                        newest_published_at,
                    ))
                } else {
                    // Instagram/Facebook/Twitter/TikTok: combine all posts then extract
                    let mut combined_text: String = posts
                        .iter()
                        .enumerate()
                        .map(|(i, p)| post_header(i, p))
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
                            warn!(source_url, error = %e, "Social extraction failed");
                            return None;
                        }
                    };
                    info!(source_url, posts = post_count, "Social scrape complete");
                    Some((
                        canonical_key,
                        source_url,
                        platform,
                        combined_text,
                        result.nodes,
                        result.resource_tags,
                        result.signal_tags,
                        result.author_actors.into_iter().collect(),
                        post_count,
                        source_mentions,
                        newest_published_at,
                    ))
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
            let (
                canonical_key,
                source_url,
                result_platform,
                combined_text,
                mut nodes,
                resource_tags,
                signal_tags,
                author_actors,
                post_count,
                mentions,
                newest_published_at,
            ) = result;

            // Apply social published_at as fallback published_at when LLM didn't extract one
            if let Some(pub_at) = newest_published_at {
                for node in &mut nodes {
                    if let Some(meta) = node.meta_mut() {
                        if meta.published_at.is_none() {
                            meta.published_at = Some(pub_at);
                        }
                    }
                }
            }

            // Accumulate mentions as URLs for promotion (capped per source)
            for handle in mentions.into_iter().take(promotion_config.max_per_source) {
                let mention_url = link_promoter::platform_url(&result_platform, &handle);
                output.collected_links.push(CollectedLink {
                    url: mention_url,
                    discovered_on: source_url.clone(),
                });
            }

            run_log.log(EventKind::SocialScrape {
                platform: "social".to_string(),
                identifier: source_url.clone(),
                post_count: post_count as u32,
            });

            run_log.log(EventKind::LlmExtraction {
                source_url: source_url.clone(),
                content_chars: combined_text.len(),
                signals_extracted: nodes.len() as u32,
                implied_queries: 0,
            });

            // Collect implied queries from Tension/Need social signals
            for node in &nodes {
                if matches!(node.node_type(), NodeType::Tension | NodeType::Need) {
                    if let Some(meta) = node.meta() {
                        output.expansion_queries
                            .extend(meta.implied_queries.iter().cloned());
                    }
                }
            }
            output.stats_delta.social_media_posts += post_count as u32;
            let source_id = ck_to_source_id.get(&canonical_key).copied();
            let events = store_signals_events(
                &source_url,
                &combined_text,
                nodes,
                resource_tags,
                signal_tags,
                &author_actors,
                url_to_canonical_key,
                actor_contexts,
                source_id,
            );
            output.source_signal_counts.entry(canonical_key).or_default();
            output.events.extend(events);
        }
        output
    }
}
