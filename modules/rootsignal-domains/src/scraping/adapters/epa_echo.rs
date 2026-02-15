use anyhow::Result;
use rootsignal_core::RawPage;
use url::form_urlencoded;

/// EPA ECHO API adapter.
///
/// Queries the EPA Enforcement and Compliance History Online (ECHO) system
/// for facility information and violations.
///
/// API docs: https://echodata.epa.gov/echo/
/// Auth: None (free, public)
/// Two-step QID pattern:
///   1. GET facility info → QueryID
///   2. GET detailed facility report by QID
pub struct EpaEchoAdapter {
    client: reqwest::Client,
}

impl EpaEchoAdapter {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Fetch facility violations from EPA ECHO.
    pub async fn fetch_violations(
        &self,
        config: &serde_json::Value,
    ) -> Result<Vec<RawPage>> {
        let facility_name = config
            .get("facility_name")
            .or_else(|| config.get("query_value"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if facility_name.is_empty() {
            return Ok(vec![]);
        }

        // Step 1: Get facility info and QID
        let qid = self.get_facility_qid(facility_name).await?;

        let Some(qid) = qid else {
            tracing::info!(facility = facility_name, "No EPA ECHO facilities found");
            return Ok(vec![]);
        };

        // Step 2: Get detailed facility report
        let pages = self.get_facility_report(&qid, facility_name).await?;

        tracing::info!(
            facility = facility_name,
            records = pages.len(),
            "EPA ECHO fetch complete"
        );

        Ok(pages)
    }

    /// Step 1: Query for facilities by name, returns a QID for subsequent queries.
    async fn get_facility_qid(&self, facility_name: &str) -> Result<Option<String>> {
        let encoded_name: String = form_urlencoded::byte_serialize(facility_name.as_bytes()).collect();
        let url = format!(
            "https://echodata.epa.gov/echo/dfr_rest_services.get_facility_info?p_name={}&output=JSON",
            encoded_name
        );

        let mut retries = 0;
        let max_retries = 3;

        loop {
            let response = self.client.get(&url).send().await?;

            if response.status().is_success() {
                let data: serde_json::Value = response.json().await?;

                let qid = data
                    .pointer("/Results/QueryID")
                    .or_else(|| data.pointer("/Results/FacilityInfo/QueryID"))
                    .and_then(|v| v.as_str())
                    .map(String::from);

                return Ok(qid);
            }

            retries += 1;
            if retries >= max_retries {
                anyhow::bail!(
                    "EPA ECHO facility query failed after {} retries: {}",
                    max_retries,
                    response.status()
                );
            }

            tracing::warn!(
                facility = facility_name,
                retry = retries,
                "EPA ECHO facility query failed, retrying"
            );
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    /// Step 2: Get detailed facility report using QID.
    async fn get_facility_report(
        &self,
        qid: &str,
        facility_name: &str,
    ) -> Result<Vec<RawPage>> {
        let url = format!(
            "https://echodata.epa.gov/echo/dfr_rest_services.get_dfr?qid={}&output=JSON",
            qid
        );

        let response = self.client.get(&url).send().await?;
        let data: serde_json::Value = response.json().await?;

        let mut pages = Vec::new();

        // Extract facilities array
        let facilities = data
            .pointer("/Results/Facilities")
            .and_then(|v| v.as_array());

        if let Some(facilities) = facilities {
            for facility in facilities {
                let frs_id = facility
                    .get("FRSFacilityID")
                    .or_else(|| facility.get("RegistryID"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                let url = format!(
                    "https://echo.epa.gov/detailed-facility-report?fid={}",
                    frs_id
                );

                let content = serde_json::to_string_pretty(facility)?;

                let page = RawPage::new(&url, &content)
                    .with_title(format!(
                        "EPA ECHO: {} (FRS {})",
                        facility_name, frs_id
                    ))
                    .with_content_type("application/json")
                    .with_metadata(
                        "source",
                        serde_json::Value::String("epa_echo".into()),
                    )
                    .with_metadata(
                        "frs_id",
                        serde_json::Value::String(frs_id.into()),
                    );

                pages.push(page);
            }
        }

        Ok(pages)
    }
}
