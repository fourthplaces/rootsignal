use apify_client::ApifyClient;
use async_trait::async_trait;
use rootsignal_core::error::{CrawlError, CrawlResult};
use rootsignal_core::ingestor::DiscoverConfig;
use rootsignal_core::types::RawPage;
use rootsignal_core::Ingestor;

pub struct TikTokIngestor {
    client: ApifyClient,
}

impl TikTokIngestor {
    pub fn new(api_key: String) -> Self {
        Self {
            client: ApifyClient::new(api_key),
        }
    }
}

#[async_trait]
impl Ingestor for TikTokIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>> {
        let handle = config
            .options
            .get("handle")
            .map(|s| s.as_str())
            .unwrap_or(&config.url);

        let limit = config.limit as u32;

        let posts = self
            .client
            .scrape_tiktok_posts(handle, limit)
            .await
            .map_err(|e| CrawlError::Http(Box::new(e)))?;

        let pages = posts
            .into_iter()
            .map(|post| {
                let content = post.text.clone().unwrap_or_default();
                let url = post
                    .web_video_url
                    .clone()
                    .unwrap_or_else(|| format!("https://tiktok.com/@{}", handle));

                let mut page = RawPage::new(url, &content)
                    .with_content_type("social/tiktok")
                    .with_metadata("platform", serde_json::Value::String("tiktok".into()));

                if let Some(author) = &post.author_meta {
                    if let Some(name) = &author.name {
                        page =
                            page.with_metadata("handle", serde_json::Value::String(name.clone()));
                    }
                    if let Some(nick) = &author.nick_name {
                        page = page
                            .with_metadata("display_name", serde_json::Value::String(nick.clone()));
                    }
                }
                if let Some(ts) = &post.create_time_iso {
                    page = page.with_metadata("posted_at", serde_json::Value::String(ts.clone()));
                }
                if let Some(likes) = post.digg_count {
                    page = page.with_metadata("likes", serde_json::json!(likes));
                }
                if let Some(shares) = post.share_count {
                    page = page.with_metadata("shares", serde_json::json!(shares));
                }
                if let Some(plays) = post.play_count {
                    page = page.with_metadata("plays", serde_json::json!(plays));
                }
                if let Some(comments) = post.comment_count {
                    page = page.with_metadata("comments", serde_json::json!(comments));
                }
                if let Some(hashtags) = &post.hashtags {
                    let tags: Vec<String> =
                        hashtags.iter().filter_map(|h| h.name.clone()).collect();
                    if !tags.is_empty() {
                        page = page.with_metadata("hashtags", serde_json::json!(tags));
                    }
                }
                page
            })
            .collect();

        Ok(pages)
    }

    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>> {
        tracing::warn!(
            count = urls.len(),
            "fetch_specific not supported for TikTok; use discover()"
        );
        Ok(Vec::new())
    }

    fn name(&self) -> &str {
        "apify_tiktok"
    }
}
