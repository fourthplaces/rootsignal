// RSS/Atom feed service.
// Returns universal ArchivedFeed content type.

use std::time::Duration;

use anyhow::{Context, Result};
use rootsignal_common::FeedItem;
use tracing::info;
use uuid::Uuid;

use crate::store::InsertFeed;

const RSS_MAX_ITEMS: usize = 20;
const RSS_MAX_AGE_DAYS: i64 = 30;

pub(crate) struct FetchedFeed {
    pub feed: InsertFeed,
}

pub(crate) struct FeedService {
    client: reqwest::Client,
}

impl FeedService {
    pub(crate) fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Failed to build RSS HTTP client");
        Self { client }
    }

    /// Fetch and parse an RSS/Atom/JSON feed, returning an InsertFeed.
    pub(crate) async fn fetch(
        &self,
        feed_url: &str,
        source_id: Uuid,
    ) -> Result<FetchedFeed> {
        let resp = self
            .client
            .get(feed_url)
            .header("User-Agent", "rootsignal-archive/0.1")
            .send()
            .await
            .context("RSS feed fetch failed")?;

        let bytes = resp.bytes().await.context("Failed to read RSS feed body")?;
        let feed = feed_rs::parser::parse(&bytes[..]).context("Failed to parse RSS/Atom feed")?;

        let cutoff = chrono::Utc::now() - chrono::Duration::days(RSS_MAX_AGE_DAYS);

        let mut items: Vec<FeedItem> = feed
            .entries
            .into_iter()
            .filter_map(|entry| {
                let url = entry
                    .links
                    .first()
                    .map(|l| l.href.clone())
                    .or_else(|| entry.id.starts_with("http").then(|| entry.id.clone()))?;

                let pub_date = entry
                    .published
                    .or(entry.updated)
                    .map(|dt| dt.with_timezone(&chrono::Utc));

                if let Some(date) = pub_date {
                    if date < cutoff {
                        return None;
                    }
                }

                Some(FeedItem {
                    url,
                    title: entry.title.map(|t| t.content),
                    pub_date,
                })
            })
            .collect();

        items.sort_by(|a, b| b.pub_date.cmp(&a.pub_date));
        items.truncate(RSS_MAX_ITEMS);

        let title = feed.title.map(|t| t.content);

        let items_json = serde_json::to_value(&items).unwrap_or(serde_json::Value::Array(vec![]));
        let content_hash =
            rootsignal_common::content_hash(&serde_json::to_string(&items_json).unwrap_or_default())
                .to_string();

        info!(feed_url, items = items.len(), "feed: parsed successfully");

        Ok(FetchedFeed {
            feed: InsertFeed {
                source_id,
                content_hash,
                items: items_json,
                title,
            },
        })
    }

    /// Discover RSS/Atom feed URLs from a webpage's HTML.
    pub(crate) fn discover_feed_urls(html: &str, base_url: &str) -> Vec<String> {
        let mut feeds = Vec::new();
        let pattern = regex::Regex::new(
            r#"<link[^>]+type\s*=\s*["']application/(rss\+xml|atom\+xml)["'][^>]*>"#,
        )
        .expect("Invalid RSS link regex");

        let href_pattern =
            regex::Regex::new(r#"href\s*=\s*["']([^"']+)["']"#).expect("Invalid href regex");

        for cap in pattern.captures_iter(html) {
            let tag = cap.get(0).map(|m| m.as_str()).unwrap_or("");
            if let Some(href_cap) = href_pattern.captures(tag) {
                if let Some(href) = href_cap.get(1) {
                    let href_str = href.as_str();
                    let full_url = if href_str.starts_with("http") {
                        href_str.to_string()
                    } else if href_str.starts_with('/') {
                        if let Ok(base) = url::Url::parse(base_url) {
                            format!(
                                "{}://{}{}",
                                base.scheme(),
                                base.host_str().unwrap_or(""),
                                href_str
                            )
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    };
                    feeds.push(full_url);
                }
            }
        }

        feeds
    }
}
