use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use rand::Rng;
use spider_transformations::transformation::content::{
    transform_content_input, ReturnFormat, TransformConfig, TransformInput,
};
use tokio::sync::Semaphore;
use tracing::{info, warn};

// --- PageScraper trait ---

#[async_trait]
pub trait PageScraper: Send + Sync {
    async fn scrape(&self, url: &str) -> Result<String>;
    /// Return raw HTML without Readability extraction. Used for query sources
    /// where we need to extract links from the page structure.
    async fn scrape_raw(&self, url: &str) -> Result<String> {
        self.scrape(url).await
    }
    fn name(&self) -> &str;
}

// --- Chrome + Readability scraper ---

/// Scraper that uses headless Chromium --dump-dom for JS rendering, then
/// spider_transformations Readability extraction for clean main content.
/// Max concurrent Chromium processes. Each instance is heavy (~100MB+ RSS,
/// multiple child processes). Railway containers hit PID/memory limits fast.
const MAX_CONCURRENT_CHROME: usize = 2;

/// Max retry attempts for transient Chrome failures (e.g. "Cannot fork").
const CHROME_MAX_ATTEMPTS: u32 = 3;
/// Base backoff duration for Chrome retries. Actual delay is base * 3^attempt + jitter.
const CHROME_RETRY_BASE: Duration = Duration::from_secs(3);

pub struct ChromeScraper {
    semaphore: Semaphore,
}

impl ChromeScraper {
    pub fn new() -> Self {
        info!("Using ChromeScraper (dump-dom + Readability extraction, max_concurrent={MAX_CONCURRENT_CHROME})");
        Self {
            semaphore: Semaphore::new(MAX_CONCURRENT_CHROME),
        }
    }

    /// Launch Chrome --dump-dom and return raw stdout bytes.
    /// Retries up to CHROME_MAX_ATTEMPTS on transient fork/launch failures
    /// with exponential backoff (3s, 9s) plus random jitter (0-1s).
    async fn run_chrome(&self, url: &str) -> Result<Vec<u8>> {
        let parsed = url::Url::parse(url).context("Invalid URL")?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            anyhow::bail!("Only http/https URLs are allowed, got: {}", parsed.scheme());
        }

        let chrome_bin = std::env::var("CHROME_BIN").unwrap_or_else(|_| "chromium".to_string());

        for attempt in 0..CHROME_MAX_ATTEMPTS {
            let tmp_dir = tempfile::tempdir().context("Failed to create temp profile dir")?;

            let result = tokio::time::timeout(
                Duration::from_secs(30),
                tokio::process::Command::new(&chrome_bin)
                    .args([
                        "--headless",
                        "--no-sandbox",
                        "--disable-gpu",
                        "--disable-dev-shm-usage",
                        &format!("--user-data-dir={}", tmp_dir.path().display()),
                        "--dump-dom",
                        url,
                    ])
                    .output(),
            )
            .await;

            match result {
                Ok(Ok(output)) => {
                    if output.status.success() {
                        if output.stdout.is_empty() {
                            if attempt + 1 < CHROME_MAX_ATTEMPTS {
                                let backoff = CHROME_RETRY_BASE * 3u32.pow(attempt);
                                let jitter =
                                    Duration::from_millis(rand::rng().random_range(0..1000));
                                warn!(
                                    url,
                                    attempt = attempt + 1,
                                    backoff_secs = backoff.as_secs(),
                                    "Chrome returned empty DOM, retrying after backoff"
                                );
                                tokio::time::sleep(backoff + jitter).await;
                                continue;
                            }
                        }
                        return Ok(output.stdout);
                    }
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    // Transient fork/resource exhaustion — retry
                    if stderr.contains("Cannot fork")
                        || stderr.contains("Resource temporarily unavailable")
                    {
                        if attempt + 1 < CHROME_MAX_ATTEMPTS {
                            let backoff = CHROME_RETRY_BASE * 3u32.pow(attempt);
                            let jitter = Duration::from_millis(rand::rng().random_range(0..1000));
                            warn!(
                                url,
                                attempt = attempt + 1,
                                backoff_secs = backoff.as_secs(),
                                "Chrome cannot fork, retrying after backoff"
                            );
                            tokio::time::sleep(backoff + jitter).await;
                            continue;
                        }
                    }
                    warn!(url, scraper = "chrome", stderr = %stderr, "Chrome exited with error");
                    return Ok(Vec::new());
                }
                Ok(Err(e)) => {
                    // Failed to launch process at all — retry on transient errors
                    let msg = e.to_string();
                    if (msg.contains("Cannot fork")
                        || msg.contains("Resource temporarily unavailable"))
                        && attempt + 1 < CHROME_MAX_ATTEMPTS
                    {
                        let backoff = CHROME_RETRY_BASE * 3u32.pow(attempt);
                        let jitter = Duration::from_millis(rand::rng().random_range(0..1000));
                        warn!(
                            url, attempt = attempt + 1, backoff_secs = backoff.as_secs(),
                            error = %e, "Chrome launch failed, retrying after backoff"
                        );
                        tokio::time::sleep(backoff + jitter).await;
                        continue;
                    }
                    anyhow::bail!("Failed to run Chrome for {url}: {e}");
                }
                Err(_) => {
                    if attempt + 1 < CHROME_MAX_ATTEMPTS {
                        let backoff = CHROME_RETRY_BASE * 3u32.pow(attempt);
                        let jitter = Duration::from_millis(rand::rng().random_range(0..1000));
                        warn!(
                            url,
                            attempt = attempt + 1,
                            backoff_secs = backoff.as_secs(),
                            "Chrome timed out, retrying after backoff"
                        );
                        tokio::time::sleep(backoff + jitter).await;
                        continue;
                    }
                    anyhow::bail!("Chrome timed out after 30s for {url}");
                }
            }
        }

        Ok(Vec::new())
    }
}

#[async_trait]
impl PageScraper for ChromeScraper {
    async fn scrape(&self, url: &str) -> Result<String> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("Chrome semaphore closed"))?;

        info!(url, scraper = "chrome", "Scraping URL");

        let html = self.run_chrome(url).await?;

        if html.is_empty() {
            warn!(url, scraper = "chrome", "Empty DOM output");
            return Ok(String::new());
        }

        let parsed_url = url::Url::parse(url).ok();
        let config = TransformConfig {
            readability: true,
            main_content: true,
            return_format: ReturnFormat::Markdown,
            filter_images: true,
            filter_svg: true,
            clean_html: true,
        };
        let input = TransformInput {
            url: parsed_url.as_ref(),
            content: &html,
            screenshot_bytes: None,
            encoding: None,
            selector_config: None,
            ignore_tags: None,
        };

        let text = transform_content_input(input, &config);

        if text.trim().is_empty() {
            warn!(
                url,
                scraper = "chrome",
                "Empty content after Readability extraction"
            );
            return Ok(String::new());
        }

        info!(
            url,
            scraper = "chrome",
            bytes = text.len(),
            "Scraped successfully"
        );
        Ok(text)
    }

    async fn scrape_raw(&self, url: &str) -> Result<String> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("Chrome semaphore closed"))?;

        info!(url, scraper = "chrome", "Scraping raw HTML");

        let html = self.run_chrome(url).await?;

        if html.is_empty() {
            warn!(url, scraper = "chrome", "Empty DOM output");
            return Ok(String::new());
        }

        let text = String::from_utf8_lossy(&html).into_owned();
        info!(
            url,
            scraper = "chrome",
            bytes = text.len(),
            "Raw HTML scraped"
        );
        Ok(text)
    }

    fn name(&self) -> &str {
        "chrome"
    }
}

// --- Browserless + Readability scraper ---

pub struct BrowserlessScraper {
    client: browserless_client::BrowserlessClient,
}

impl BrowserlessScraper {
    pub fn new(base_url: &str, token: Option<&str>) -> Self {
        info!(base_url, "Using BrowserlessScraper");
        Self {
            client: browserless_client::BrowserlessClient::new(base_url, token),
        }
    }
}

#[async_trait]
impl PageScraper for BrowserlessScraper {
    async fn scrape(&self, url: &str) -> Result<String> {
        info!(url, scraper = "browserless", "Scraping URL");

        let html = self
            .client
            .content(url)
            .await
            .context("Browserless content request failed")?;

        if html.is_empty() {
            warn!(url, scraper = "browserless", "Empty HTML response");
            return Ok(String::new());
        }

        let parsed_url = url::Url::parse(url).ok();
        let config = TransformConfig {
            readability: true,
            main_content: true,
            return_format: ReturnFormat::Markdown,
            filter_images: true,
            filter_svg: true,
            clean_html: true,
        };
        let input = TransformInput {
            url: parsed_url.as_ref(),
            content: html.as_bytes(),
            screenshot_bytes: None,
            encoding: None,
            selector_config: None,
            ignore_tags: None,
        };

        let text = transform_content_input(input, &config);

        if text.trim().is_empty() {
            warn!(
                url,
                scraper = "browserless",
                "Empty content after Readability extraction"
            );
            return Ok(String::new());
        }

        info!(
            url,
            scraper = "browserless",
            bytes = text.len(),
            "Scraped successfully"
        );
        Ok(text)
    }

    async fn scrape_raw(&self, url: &str) -> Result<String> {
        info!(url, scraper = "browserless", "Scraping raw HTML");

        let html = self
            .client
            .content(url)
            .await
            .context("Browserless content request failed")?;

        info!(
            url,
            scraper = "browserless",
            bytes = html.len(),
            "Raw HTML scraped"
        );
        Ok(html)
    }

    fn name(&self) -> &str {
        "browserless"
    }
}

/// Extract links from raw HTML that match a given URL pattern.
/// Resolves relative URLs against `base_url`, deduplicates, and caps at 20 results.
pub fn extract_links_by_pattern(html: &str, base_url: &str, pattern: &str) -> Vec<String> {
    let href_re = regex::Regex::new(r#"href\s*=\s*["']([^"']+)["']"#).expect("valid regex");
    let base = url::Url::parse(base_url).ok();

    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    for cap in href_re.captures_iter(html) {
        let raw = &cap[1];

        // Resolve relative URLs
        let resolved = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw.to_string()
        } else if let Some(ref b) = base {
            match b.join(raw) {
                Ok(u) => u.to_string(),
                Err(_) => continue,
            }
        } else {
            continue;
        };

        if resolved.contains(pattern) && seen.insert(resolved.clone()) {
            links.push(resolved);
            if links.len() >= 20 {
                break;
            }
        }
    }

    links
}

// --- Social media types ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocialPlatform {
    Instagram,
    Facebook,
    Reddit,
    Twitter,
    TikTok,
}

#[derive(Debug, Clone)]
pub struct SocialAccount {
    pub platform: SocialPlatform,
    pub identifier: String,
}

#[derive(Debug, Clone)]
pub struct SocialPost {
    pub content: String,
    pub author: Option<String>,
    pub url: Option<String>,
}

// --- WebSearcher trait ---

#[async_trait]
pub trait WebSearcher: Send + Sync {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>>;
}

// --- SocialScraper trait ---

#[async_trait]
pub trait SocialScraper: Send + Sync {
    async fn search_posts(&self, account: &SocialAccount, limit: u32) -> Result<Vec<SocialPost>>;
    async fn search_hashtags(&self, hashtags: &[&str], limit: u32) -> Result<Vec<SocialPost>>;
    /// Search a specific platform for topics (hashtags/keywords). Used by multi-platform
    /// topic discovery. Returns empty vec for unsupported platforms.
    async fn search_topics(
        &self,
        platform: &SocialPlatform,
        topics: &[&str],
        limit: u32,
    ) -> Result<Vec<SocialPost>>;
    /// Search GoFundMe campaigns by keyword. Returns campaign data for signal extraction.
    async fn search_gofundme(
        &self,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<apify_client::GoFundMeCampaign>>;
}

/// No-op social scraper for when no API key is configured.
pub struct NoopSocialScraper;

#[async_trait]
impl SocialScraper for NoopSocialScraper {
    async fn search_posts(&self, _account: &SocialAccount, _limit: u32) -> Result<Vec<SocialPost>> {
        Ok(Vec::new())
    }

    async fn search_hashtags(&self, _hashtags: &[&str], _limit: u32) -> Result<Vec<SocialPost>> {
        Ok(Vec::new())
    }

    async fn search_topics(
        &self,
        _platform: &SocialPlatform,
        _topics: &[&str],
        _limit: u32,
    ) -> Result<Vec<SocialPost>> {
        Ok(Vec::new())
    }

    async fn search_gofundme(
        &self,
        _keyword: &str,
        _limit: u32,
    ) -> Result<Vec<apify_client::GoFundMeCampaign>> {
        Ok(Vec::new())
    }
}

// --- Serper (Google Search) ---

pub struct SerperSearcher {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, serde::Deserialize)]
struct SerperResponse {
    #[serde(default)]
    organic: Vec<SerperResult>,
}

#[derive(Debug, serde::Deserialize)]
struct SerperResult {
    #[serde(default)]
    link: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    snippet: String,
}

impl SerperSearcher {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
        }
    }
}

#[async_trait]
impl WebSearcher for SerperSearcher {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        info!(query, max_results, "Serper search");

        let body = serde_json::json!({
            "q": query,
            "num": max_results,
        });

        let resp = self
            .client
            .post("https://google.serper.dev/search")
            .header("X-API-KEY", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Serper API request failed")?;

        let data: SerperResponse = resp
            .json()
            .await
            .context("Failed to parse Serper response")?;

        let results: Vec<SearchResult> = data
            .organic
            .into_iter()
            .map(|r| SearchResult {
                url: r.link,
                title: r.title,
                snippet: r.snippet,
            })
            .collect();

        info!(query, count = results.len(), "Serper search complete");
        Ok(results)
    }
}

// --- SocialScraper impl for ApifyClient ---

use apify_client::ApifyClient;

#[async_trait]
impl SocialScraper for ApifyClient {
    async fn search_posts(&self, account: &SocialAccount, limit: u32) -> Result<Vec<SocialPost>> {
        match account.platform {
            SocialPlatform::Instagram => {
                let posts = self
                    .scrape_instagram_posts(&account.identifier, limit)
                    .await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        let content = p.caption?;
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: p.owner_username,
                            url: Some(p.url),
                        })
                    })
                    .collect())
            }
            SocialPlatform::Facebook => {
                let posts = self
                    .scrape_facebook_posts(&account.identifier, limit)
                    .await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        let content = p.text?;
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: p.page_name,
                            url: p.url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::Reddit => {
                let posts = self.scrape_reddit_posts(&account.identifier, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        // Skip comments and community info — only keep actual posts
                        if p.data_type.as_deref() != Some("post") {
                            return None;
                        }
                        let title = p.title.unwrap_or_default();
                        let body = p.body.unwrap_or_default();
                        let content = format!("{}\n\n{}", title, body).trim().to_string();
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: None,
                            url: p.url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::Twitter => {
                let tweets = self.scrape_x_posts(&account.identifier, limit).await?;
                Ok(tweets
                    .into_iter()
                    .filter_map(|t| {
                        let content = t.content()?.to_string();
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: t.author.as_ref().and_then(|a| a.user_name.clone()),
                            url: t.url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::TikTok => {
                let posts = self.scrape_tiktok_posts(&account.identifier, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        let content = p.text?;
                        if content.len() < 20 {
                            return None; // Skip sparse captions (same filter as search_topics)
                        }
                        Some(SocialPost {
                            content,
                            author: p.author_meta.as_ref().and_then(|a| a.name.clone()),
                            url: p.web_video_url,
                        })
                    })
                    .collect())
            }
        }
    }

    async fn search_hashtags(&self, hashtags: &[&str], limit: u32) -> Result<Vec<SocialPost>> {
        let posts = self.search_instagram_hashtags(hashtags, limit).await?;
        Ok(posts
            .into_iter()
            .map(|p| SocialPost {
                content: p.content,
                author: Some(p.author_username),
                url: Some(p.post_url),
            })
            .collect())
    }

    async fn search_gofundme(
        &self,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<apify_client::GoFundMeCampaign>> {
        self.scrape_gofundme(keyword, limit)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    async fn search_topics(
        &self,
        platform: &SocialPlatform,
        topics: &[&str],
        limit: u32,
    ) -> Result<Vec<SocialPost>> {
        match platform {
            SocialPlatform::Instagram => self.search_hashtags(topics, limit).await,
            SocialPlatform::Twitter => {
                let posts = self.search_x_keywords(topics, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|t| {
                        let content = t.content()?.to_string();
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: t.author.as_ref().and_then(|a| a.user_name.clone()),
                            url: t.url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::TikTok => {
                let posts = self.search_tiktok_keywords(topics, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        let content = p.text?;
                        if content.len() < 20 {
                            return None; // Skip sparse captions
                        }
                        Some(SocialPost {
                            content,
                            author: p.author_meta.as_ref().and_then(|a| a.name.clone()),
                            url: p.web_video_url,
                        })
                    })
                    .collect())
            }
            // Facebook and Reddit don't support keyword search
            _ => Ok(Vec::new()),
        }
    }
}
