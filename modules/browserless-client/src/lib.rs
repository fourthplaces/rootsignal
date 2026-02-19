pub mod error;

pub use error::{BrowserlessError, Result};

use std::time::Duration;

pub struct BrowserlessClient {
    client: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

impl BrowserlessClient {
    pub fn new(base_url: &str, token: Option<&str>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(String::from),
        }
    }

    /// Fetch fully-rendered HTML content for a URL via Browserless /content endpoint.
    pub async fn content(&self, url: &str) -> Result<String> {
        let mut endpoint = format!("{}/content", self.base_url);
        if let Some(ref token) = self.token {
            endpoint.push_str(&format!("?token={token}"));
        }

        let body = serde_json::json!({ "url": url });

        let resp = self
            .client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(BrowserlessError::Api {
                status: status.as_u16(),
                message,
            });
        }

        Ok(resp.text().await?)
    }
}
