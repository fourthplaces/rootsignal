use anyhow::Result;
use chrono::Utc;
use rootsignal_core::RawPage;

/// USAspending.gov API adapter.
///
/// Queries the USAspending API for federal awards by recipient name.
/// Returns each award as a RawPage with JSON content for LLM signal extraction.
///
/// API docs: https://api.usaspending.gov/
/// Auth: None (free, public)
/// Rate limit: 30 req/min with exponential backoff on 429
pub struct UsaSpendingAdapter {
    client: reqwest::Client,
}

impl UsaSpendingAdapter {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Fetch awards for a recipient from USAspending.
    pub async fn fetch_awards(&self, config: &serde_json::Value) -> Result<Vec<RawPage>> {
        let recipient_name = config
            .get("query_value")
            .or_else(|| config.get("recipient_name"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if recipient_name.is_empty() {
            return Ok(vec![]);
        }

        let date_range_start = config
            .get("date_range_start")
            .and_then(|v| v.as_str())
            .unwrap_or("2015-01-01");

        let mut pages = Vec::new();
        let mut page_num = 1;
        let page_size = 100;

        loop {
            let body = serde_json::json!({
                "filters": {
                    "recipient_search_text": [recipient_name],
                    "time_period": [{
                        "start_date": date_range_start,
                        "end_date": Utc::now().format("%Y-%m-%d").to_string()
                    }]
                },
                "fields": [
                    "Award ID", "Recipient Name", "Award Amount",
                    "Awarding Agency", "Awarding Sub Agency",
                    "Award Type", "Start Date", "End Date",
                    "Description", "generated_internal_id"
                ],
                "page": page_num,
                "limit": page_size,
                "sort": "Award Amount",
                "order": "desc"
            });

            let response = self
                .client
                .post("https://api.usaspending.gov/api/v2/search/spending_by_award/")
                .json(&body)
                .send()
                .await?;

            // Handle rate limiting
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                tracing::warn!("USAspending rate limited, backing off");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }

            let data: serde_json::Value = response.json().await?;

            let results = data["results"].as_array();
            let result_count = results.map(|r| r.len()).unwrap_or(0);

            if let Some(results) = results {
                for award in results {
                    let award_id = award["Award ID"].as_str().unwrap_or("unknown");
                    let internal_id = award["generated_internal_id"].as_str().unwrap_or(award_id);

                    let url = format!("https://www.usaspending.gov/award/{}", internal_id);

                    let content = serde_json::to_string_pretty(award)?;

                    let page = RawPage::new(&url, &content)
                        .with_title(format!("USAspending Award: {}", award_id))
                        .with_content_type("application/json")
                        .with_metadata("source", serde_json::Value::String("usaspending".into()))
                        .with_metadata("award_id", serde_json::Value::String(award_id.into()));

                    pages.push(page);
                }
            }

            // Stop if we got less than a full page
            if result_count < page_size {
                break;
            }

            page_num += 1;

            // Safety limit: don't fetch more than 10 pages (1000 awards)
            if page_num > 10 {
                tracing::info!("USAspending: hit 10-page limit, stopping pagination");
                break;
            }

            // Rate limit: ~2 req/sec
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        tracing::info!(
            recipient = recipient_name,
            awards = pages.len(),
            "USAspending fetch complete"
        );

        Ok(pages)
    }
}
