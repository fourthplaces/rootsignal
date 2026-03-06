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
    pub(crate) async fn fetch(&self, feed_url: &str, source_id: Uuid) -> Result<FetchedFeed> {
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
        let content_hash = rootsignal_common::content_hash(
            &serde_json::to_string(&items_json).unwrap_or_default(),
        )
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
        use scraper::{Html, Selector};

        let document = Html::parse_document(html);
        let selector = Selector::parse(
            r#"head link[type="application/rss+xml"], head link[type="application/atom+xml"]"#,
        )
        .expect("valid CSS selector");

        let base = url::Url::parse(base_url).ok();

        document
            .select(&selector)
            .filter_map(|el| {
                let href = el.value().attr("href")?;
                if href.starts_with("http://") || href.starts_with("https://") {
                    Some(href.to_string())
                } else {
                    Some(base.as_ref()?.join(href).ok()?.to_string())
                }
            })
            .collect()
    }
}
