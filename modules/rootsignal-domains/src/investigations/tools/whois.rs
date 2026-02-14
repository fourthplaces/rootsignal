use ai_client::tool::ToolDefinition;
use ai_client::Tool;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct WhoisArgs {
    pub domain: String,
}

#[derive(Debug, Serialize)]
pub struct WhoisOutput {
    pub domain: String,
    pub registered_date: Option<String>,
    pub registrar: Option<String>,
    pub domain_age_days: Option<i64>,
}

pub struct WhoisLookupTool {
    client: reqwest::Client,
}

impl WhoisLookupTool {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[derive(Debug)]
pub struct WhoisError(anyhow::Error);

impl std::fmt::Display for WhoisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for WhoisError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

#[derive(Debug, Deserialize)]
struct RdapResponse {
    events: Option<Vec<RdapEvent>>,
    entities: Option<Vec<RdapEntity>>,
}

#[derive(Debug, Deserialize)]
struct RdapEvent {
    #[serde(rename = "eventAction")]
    event_action: String,
    #[serde(rename = "eventDate")]
    event_date: String,
}

#[derive(Debug, Deserialize)]
struct RdapEntity {
    roles: Option<Vec<String>>,
    #[serde(rename = "publicIds")]
    public_ids: Option<Vec<RdapPublicId>>,
    handle: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RdapPublicId {
    identifier: String,
}

#[async_trait]
impl Tool for WhoisLookupTool {
    const NAME: &'static str = "whois_lookup";
    type Error = WhoisError;
    type Args = WhoisArgs;
    type Output = WhoisOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Look up WHOIS/RDAP information for a domain to determine registration date, registrar, and domain age.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "The domain name to look up (e.g. example.com)"
                    }
                },
                "required": ["domain"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let domain = args
            .domain
            .trim()
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .split('/')
            .next()
            .unwrap_or(&args.domain);

        let url = format!("https://rdap.org/domain/{}", domain);

        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/rdap+json")
            .send()
            .await
            .map_err(|e| WhoisError(e.into()))?;

        if !resp.status().is_success() {
            return Ok(WhoisOutput {
                domain: domain.to_string(),
                registered_date: None,
                registrar: None,
                domain_age_days: None,
            });
        }

        let rdap: RdapResponse = resp.json().await.map_err(|e| WhoisError(e.into()))?;

        let registered_date = rdap.events.as_ref().and_then(|events| {
            events
                .iter()
                .find(|e| e.event_action == "registration")
                .map(|e| e.event_date.clone())
        });

        let domain_age_days = registered_date.as_ref().and_then(|date_str| {
            chrono::DateTime::parse_from_rfc3339(date_str)
                .ok()
                .map(|dt| (chrono::Utc::now() - dt.with_timezone(&chrono::Utc)).num_days())
        });

        let registrar = rdap.entities.as_ref().and_then(|entities| {
            entities
                .iter()
                .find(|e| {
                    e.roles
                        .as_ref()
                        .map(|r| r.contains(&"registrar".to_string()))
                        .unwrap_or(false)
                })
                .and_then(|e| {
                    e.public_ids
                        .as_ref()
                        .and_then(|ids| ids.first().map(|id| id.identifier.clone()))
                        .or_else(|| e.handle.clone())
                })
        });

        Ok(WhoisOutput {
            domain: domain.to_string(),
            registered_date,
            registrar,
            domain_age_days,
        })
    }
}
