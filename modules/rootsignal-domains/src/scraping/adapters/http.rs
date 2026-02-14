use async_trait::async_trait;
use std::collections::{HashSet, VecDeque};
use rootsignal_core::error::{CrawlError, CrawlResult};
use rootsignal_core::{DiscoverConfig, Ingestor, RawPage};
use url::Url;

/// HTTP ingestor with BFS link-following, HTML-to-markdown, and rate limiting.
pub struct HttpIngestor {
    client: reqwest::Client,
    user_agent: String,
    rate_limit_ms: u64,
}

impl HttpIngestor {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            user_agent: "RootSignalBot/1.0".to_string(),
            rate_limit_ms: 200,
        }
    }

    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    pub fn with_rate_limit_ms(mut self, ms: u64) -> Self {
        self.rate_limit_ms = ms;
        self
    }

    /// Extract links from HTML content.
    fn extract_links(&self, base_url: &Url, html: &str) -> Vec<String> {
        let href_pattern = regex::Regex::new(r#"href\s*=\s*["']([^"']+)["']"#).unwrap();
        let mut links = Vec::new();

        for cap in href_pattern.captures_iter(html) {
            if let Some(href) = cap.get(1) {
                let href = href.as_str();
                if href.starts_with('#')
                    || href.starts_with("javascript:")
                    || href.starts_with("mailto:")
                    || href.starts_with("tel:")
                {
                    continue;
                }
                if let Ok(resolved) = base_url.join(href) {
                    links.push(resolved.to_string());
                }
            }
        }

        links
    }

    /// Check if a URL should be crawled based on config.
    fn should_crawl(&self, url: &Url, base_url: &Url, config: &DiscoverConfig) -> bool {
        let base_host = base_url.host_str().unwrap_or("");
        let url_host = url.host_str().unwrap_or("");

        // Must be same host
        if url_host != base_host {
            return false;
        }

        let path = url.path();

        // Check include patterns
        if !config.include_patterns.is_empty() {
            let matches = config.include_patterns.iter().any(|p| path.contains(p.as_str()));
            if !matches {
                return false;
            }
        }

        // Check exclude patterns
        if !config.exclude_patterns.is_empty() {
            let excluded = config.exclude_patterns.iter().any(|p| path.contains(p.as_str()));
            if excluded {
                return false;
            }
        }

        true
    }

    /// Convert HTML to markdown (simplified).
    fn html_to_markdown(&self, html: &str) -> String {
        let mut text = html.to_string();

        // Remove scripts and styles
        let script_pattern = regex::Regex::new(r"(?s)<script[^>]*>.*?</script>").unwrap();
        let style_pattern = regex::Regex::new(r"(?s)<style[^>]*>.*?</style>").unwrap();
        text = script_pattern.replace_all(&text, "").to_string();
        text = style_pattern.replace_all(&text, "").to_string();

        // Convert headers
        let h1 = regex::Regex::new(r"<h1[^>]*>(.*?)</h1>").unwrap();
        let h2 = regex::Regex::new(r"<h2[^>]*>(.*?)</h2>").unwrap();
        let h3 = regex::Regex::new(r"<h3[^>]*>(.*?)</h3>").unwrap();
        text = h1.replace_all(&text, "# $1\n").to_string();
        text = h2.replace_all(&text, "## $1\n").to_string();
        text = h3.replace_all(&text, "### $1\n").to_string();

        // Convert paragraphs and line breaks
        let p = regex::Regex::new(r"<p[^>]*>(.*?)</p>").unwrap();
        let br = regex::Regex::new(r"<br\s*/?>").unwrap();
        text = p.replace_all(&text, "$1\n\n").to_string();
        text = br.replace_all(&text, "\n").to_string();

        // Convert links
        let link = regex::Regex::new(r#"<a[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#).unwrap();
        text = link.replace_all(&text, "[$2]($1)").to_string();

        // Convert lists
        let li = regex::Regex::new(r"<li[^>]*>(.*?)</li>").unwrap();
        text = li.replace_all(&text, "- $1\n").to_string();

        // Remove remaining tags
        let tag = regex::Regex::new(r"<[^>]+>").unwrap();
        text = tag.replace_all(&text, "").to_string();

        // Clean up whitespace
        let multi_newline = regex::Regex::new(r"\n{3,}").unwrap();
        text = multi_newline.replace_all(&text, "\n\n").to_string();

        // Decode HTML entities
        text = text
            .replace("&nbsp;", " ")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'");

        text.trim().to_string()
    }

    /// Extract title from HTML.
    fn extract_title(&self, html: &str) -> Option<String> {
        let title_pattern = regex::Regex::new(r"<title[^>]*>(.*?)</title>").ok()?;
        title_pattern
            .captures(html)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().trim().to_string())
    }

    /// Fetch a single page and convert to RawPage.
    async fn fetch_page(&self, url: &str) -> CrawlResult<RawPage> {
        let response = self
            .client
            .get(url)
            .header("User-Agent", &self.user_agent)
            .send()
            .await
            .map_err(|e| CrawlError::Http(Box::new(e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(CrawlError::Http(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("HTTP {}", status),
            ))));
        }

        let html = response
            .text()
            .await
            .map_err(|e| CrawlError::Http(Box::new(e)))?;

        let title = self.extract_title(&html);
        let content = self.html_to_markdown(&html);

        let mut page = RawPage::new(url, &content)
            .with_html(html)
            .with_content_type("text/html".to_string())
            .with_metadata("fetched_via", serde_json::Value::String("http".to_string()));

        if let Some(title) = title {
            page = page.with_title(title);
        }

        Ok(page)
    }
}

#[async_trait]
impl Ingestor for HttpIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>> {
        let base_url = Url::parse(&config.url).map_err(|_| CrawlError::InvalidUrl {
            url: config.url.clone(),
        })?;

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        let mut pages: Vec<RawPage> = Vec::new();

        queue.push_back((config.url.clone(), 0));

        while let Some((url, depth)) = queue.pop_front() {
            if pages.len() >= config.limit {
                break;
            }
            if depth > config.max_depth {
                continue;
            }
            if visited.contains(&url) {
                continue;
            }

            visited.insert(url.clone());

            match self.fetch_page(&url).await {
                Ok(page) => {
                    // Extract links for further crawling from the HTML
                    if let Some(html) = &page.html {
                        if let Ok(page_url) = Url::parse(&url) {
                            let links = self.extract_links(&page_url, html);
                            for link in links {
                                if let Ok(link_url) = Url::parse(&link) {
                                    if self.should_crawl(&link_url, &base_url, config)
                                        && !visited.contains(&link)
                                    {
                                        queue.push_back((link, depth + 1));
                                    }
                                }
                            }
                        }
                    }

                    pages.push(page);
                }
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "HTTP fetch error");
                }
            }

            // Rate limiting
            if self.rate_limit_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.rate_limit_ms)).await;
            }
        }

        Ok(pages)
    }

    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>> {
        let mut pages = Vec::new();

        for url in urls {
            match self.fetch_page(url).await {
                Ok(page) => pages.push(page),
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "HTTP fetch error");
                }
            }
        }

        Ok(pages)
    }

    fn name(&self) -> &str {
        "http"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ingestor() -> HttpIngestor {
        HttpIngestor::new(reqwest::Client::new())
    }

    #[test]
    fn test_extract_links() {
        let ingestor = make_ingestor();
        let base_url = Url::parse("https://example.com/page").unwrap();

        let html = r##"
            <a href="/about">About</a>
            <a href="https://example.com/contact">Contact</a>
            <a href="#section">Anchor</a>
            <a href="javascript:void(0)">JS</a>
        "##;

        let links = ingestor.extract_links(&base_url, html);

        assert!(links.contains(&"https://example.com/about".to_string()));
        assert!(links.contains(&"https://example.com/contact".to_string()));
        assert!(!links.iter().any(|l| l.contains('#')));
        assert!(!links.iter().any(|l| l.contains("javascript")));
    }

    #[test]
    fn test_should_crawl_same_host() {
        let ingestor = make_ingestor();
        let base = Url::parse("https://example.com").unwrap();
        let config = DiscoverConfig::new("https://example.com");

        let same = Url::parse("https://example.com/page").unwrap();
        let different = Url::parse("https://other.com/page").unwrap();

        assert!(ingestor.should_crawl(&same, &base, &config));
        assert!(!ingestor.should_crawl(&different, &base, &config));
    }

    #[test]
    fn test_should_crawl_with_patterns() {
        let ingestor = make_ingestor();
        let base = Url::parse("https://example.com").unwrap();
        let config = DiscoverConfig::new("https://example.com")
            .include("/blog/")
            .exclude("/admin/");

        let blog = Url::parse("https://example.com/blog/post-1").unwrap();
        let admin = Url::parse("https://example.com/admin/settings").unwrap();
        let other = Url::parse("https://example.com/about").unwrap();

        assert!(ingestor.should_crawl(&blog, &base, &config));
        assert!(!ingestor.should_crawl(&admin, &base, &config));
        assert!(!ingestor.should_crawl(&other, &base, &config)); // doesn't match include
    }

    #[test]
    fn test_html_to_markdown() {
        let ingestor = make_ingestor();

        let html = r#"
            <h1>Title</h1>
            <p>Paragraph text.</p>
            <a href="https://example.com">Link</a>
        "#;

        let md = ingestor.html_to_markdown(html);

        assert!(md.contains("# Title"));
        assert!(md.contains("Paragraph text."));
        assert!(md.contains("[Link](https://example.com)"));
    }

    #[test]
    fn test_extract_title() {
        let ingestor = make_ingestor();

        let html = "<html><head><title>Page Title</title></head></html>";
        assert_eq!(ingestor.extract_title(html), Some("Page Title".to_string()));

        let html_no_title = "<html><body>No title</body></html>";
        assert_eq!(ingestor.extract_title(html_no_title), None);
    }
}
