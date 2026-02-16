use anyhow::Result;

/// Application configuration loaded from environment variables.
/// Contains only secrets and env-specific values; identity, models,
/// clustering params, and prompts live in the TOML FileConfig.
#[derive(Debug, Clone)]
pub struct AppConfig {
    // Database
    pub database_url: String,

    // AI / LLM
    pub openai_api_key: String,
    pub anthropic_api_key: Option<String>,

    // Scraping
    pub tavily_api_key: String,
    pub firecrawl_api_key: Option<String>,
    pub apify_api_key: Option<String>,
    pub eventbrite_api_key: Option<String>,

    // Browser (Chrome CDP for JS rendering)
    pub chrome_url: Option<String>,

    // Geocoding
    pub geocoding_api_key: Option<String>,

    // Restate
    pub restate_admin_url: Option<String>,
    pub restate_self_url: Option<String>,
    pub restate_auth_token: Option<String>,

    // Auth
    pub jwt_secret: Option<String>,
    pub twilio_account_sid: Option<String>,
    pub twilio_auth_token: Option<String>,
    pub twilio_verify_service_sid: Option<String>,
    pub admin_phone_numbers: Vec<String>,
    pub test_identifier_enabled: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        let config = Self {
            database_url: std::env::var("DATABASE_URL")?,
            openai_api_key: std::env::var("OPENAI_API_KEY")?,
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            tavily_api_key: std::env::var("TAVILY_API_KEY")?,
            chrome_url: std::env::var("CHROME_URL").ok(),
            firecrawl_api_key: std::env::var("FIRECRAWL_API_KEY").ok(),
            apify_api_key: std::env::var("APIFY_API_KEY").ok(),
            eventbrite_api_key: std::env::var("EVENTBRITE_API_KEY").ok(),
            geocoding_api_key: std::env::var("GEOCODING_API_KEY").ok(),
            restate_admin_url: std::env::var("RESTATE_ADMIN_URL").ok(),
            restate_self_url: std::env::var("RESTATE_SELF_URL").ok(),
            restate_auth_token: std::env::var("RESTATE_AUTH_TOKEN").ok(),
            jwt_secret: std::env::var("JWT_SECRET").ok(),
            twilio_account_sid: std::env::var("TWILIO_ACCOUNT_SID").ok(),
            twilio_auth_token: std::env::var("TWILIO_AUTH_TOKEN").ok(),
            twilio_verify_service_sid: std::env::var("TWILIO_VERIFY_SERVICE_SID").ok(),
            admin_phone_numbers: std::env::var("ADMIN_PHONE_NUMBERS")
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string())
                .collect(),
            test_identifier_enabled: std::env::var("TEST_IDENTIFIER_ENABLED")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
        };

        config.log_keys();
        Ok(config)
    }

    fn log_keys(&self) {
        fn preview(val: &str) -> String {
            let n = val.len().min(5);
            format!("{}...({} chars)", &val[..n], val.len())
        }
        fn preview_opt(val: &Option<String>) -> String {
            match val {
                Some(v) if !v.is_empty() => preview(v),
                _ => "<not set>".to_string(),
            }
        }

        tracing::info!("Config loaded:");
        tracing::info!("  OPENAI_API_KEY: {}", preview(&self.openai_api_key));
        tracing::info!("  ANTHROPIC_API_KEY: {}", preview_opt(&self.anthropic_api_key));
        tracing::info!("  TAVILY_API_KEY: {}", preview(&self.tavily_api_key));
        tracing::info!("  CHROME_URL: {}", preview_opt(&self.chrome_url));
        tracing::info!("  FIRECRAWL_API_KEY: {}", preview_opt(&self.firecrawl_api_key));
        tracing::info!("  APIFY_API_KEY: {}", preview_opt(&self.apify_api_key));
        tracing::info!("  RESTATE_ADMIN_URL: {}", preview_opt(&self.restate_admin_url));
    }
}
