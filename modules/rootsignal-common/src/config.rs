use std::env;

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    // Memgraph (bolt protocol via neo4rs driver)
    pub memgraph_uri: String,
    pub memgraph_user: String,
    pub memgraph_password: String,

    // AI providers
    pub anthropic_api_key: String,
    pub voyage_api_key: String,

    // Scraping
    pub tavily_api_key: String,
    pub apify_api_key: String,

    // Web server
    pub web_host: String,
    pub web_port: u16,

    // Admin
    pub admin_username: String,
    pub admin_password: String,

    // City
    pub city: String,

    // City bootstrap (optional — for cold-start or explicit override)
    pub city_name: Option<String>,
    pub city_lat: Option<f64>,
    pub city_lng: Option<f64>,
    pub city_radius_km: Option<f64>,

    // Budget
    /// Daily budget limit in cents. 0 = unlimited.
    pub daily_budget_cents: u64,

    // Twilio (for admin OTP auth)
    pub twilio_account_sid: String,
    pub twilio_auth_token: String,
    pub twilio_service_id: String,

    // Admin phone numbers (E.164) allowed to authenticate
    pub admin_numbers: Vec<String>,
}

impl Config {
    /// Load configuration from environment variables.
    /// Panics with a clear message if required vars are missing.
    pub fn from_env() -> Self {
        Self {
            memgraph_uri: required_env("MEMGRAPH_URI"),
            memgraph_user: required_env("MEMGRAPH_USER"),
            memgraph_password: required_env("MEMGRAPH_PASSWORD"),
            anthropic_api_key: required_env("ANTHROPIC_API_KEY"),
            voyage_api_key: required_env("VOYAGE_API_KEY"),
            tavily_api_key: required_env("TAVILY_API_KEY"),
            apify_api_key: env::var("APIFY_API_KEY").unwrap_or_default(),
            web_host: env::var("WEB_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            web_port: env::var("WEB_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("WEB_PORT must be a number"),
            admin_username: env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string()),
            admin_password: required_env("ADMIN_PASSWORD"),
            city: String::new(),
            city_name: None,
            city_lat: None,
            city_lng: None,
            city_radius_km: None,
            daily_budget_cents: 0,
            twilio_account_sid: String::new(),
            twilio_auth_token: String::new(),
            twilio_service_id: String::new(),
            admin_numbers: Vec::new(),
        }
    }

    /// Load config for scout (no web server or admin fields needed).
    pub fn scout_from_env() -> Self {
        Self {
            memgraph_uri: required_env("MEMGRAPH_URI"),
            memgraph_user: required_env("MEMGRAPH_USER"),
            memgraph_password: required_env("MEMGRAPH_PASSWORD"),
            anthropic_api_key: required_env("ANTHROPIC_API_KEY"),
            voyage_api_key: required_env("VOYAGE_API_KEY"),
            tavily_api_key: required_env("TAVILY_API_KEY"),
            apify_api_key: env::var("APIFY_API_KEY").unwrap_or_default(),
            web_host: String::new(),
            web_port: 0,
            admin_username: String::new(),
            admin_password: String::new(),
            city: env::var("CITY").unwrap_or_else(|_| "twincities".to_string()),
            city_name: env::var("CITY_NAME").ok(),
            city_lat: env::var("CITY_LAT").ok().and_then(|v| v.parse().ok()),
            city_lng: env::var("CITY_LNG").ok().and_then(|v| v.parse().ok()),
            city_radius_km: env::var("CITY_RADIUS_KM").ok().and_then(|v| v.parse().ok()),
            daily_budget_cents: env::var("DAILY_BUDGET_CENTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            twilio_account_sid: String::new(),
            twilio_auth_token: String::new(),
            twilio_service_id: String::new(),
            admin_numbers: Vec::new(),
        }
    }

    /// Load config for edition generation (Memgraph + Anthropic + city).
    pub fn editions_from_env() -> Self {
        Self {
            memgraph_uri: required_env("MEMGRAPH_URI"),
            memgraph_user: required_env("MEMGRAPH_USER"),
            memgraph_password: required_env("MEMGRAPH_PASSWORD"),
            anthropic_api_key: required_env("ANTHROPIC_API_KEY"),
            voyage_api_key: String::new(),
            tavily_api_key: String::new(),
            apify_api_key: String::new(),
            web_host: String::new(),
            web_port: 0,
            admin_username: String::new(),
            admin_password: String::new(),
            city: env::var("CITY").unwrap_or_else(|_| "twincities".to_string()),
            city_name: None,
            city_lat: None,
            city_lng: None,
            city_radius_km: None,
            daily_budget_cents: 0,
            twilio_account_sid: String::new(),
            twilio_auth_token: String::new(),
            twilio_service_id: String::new(),
            admin_numbers: Vec::new(),
        }
    }

    /// Load config for the scout supervisor (Memgraph + Anthropic + city + notifications).
    pub fn supervisor_from_env() -> Self {
        Self {
            memgraph_uri: required_env("MEMGRAPH_URI"),
            memgraph_user: required_env("MEMGRAPH_USER"),
            memgraph_password: required_env("MEMGRAPH_PASSWORD"),
            anthropic_api_key: required_env("ANTHROPIC_API_KEY"),
            voyage_api_key: String::new(),
            tavily_api_key: String::new(),
            apify_api_key: String::new(),
            web_host: String::new(),
            web_port: 0,
            admin_username: String::new(),
            admin_password: String::new(),
            city: env::var("CITY").unwrap_or_else(|_| "twincities".to_string()),
            city_name: None,
            city_lat: None,
            city_lng: None,
            city_radius_km: None,
            daily_budget_cents: env::var("SUPERVISOR_DAILY_BUDGET_CENTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),
            twilio_account_sid: String::new(),
            twilio_auth_token: String::new(),
            twilio_service_id: String::new(),
            admin_numbers: Vec::new(),
        }
    }

    /// Load config for the web/API server.
    /// AI keys are optional — if set, the API can trigger scout runs.
    pub fn web_from_env() -> Self {
        let admin_numbers: Vec<String> = env::var("ADMIN_NUMBERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Self {
            memgraph_uri: required_env("MEMGRAPH_URI"),
            memgraph_user: required_env("MEMGRAPH_USER"),
            memgraph_password: required_env("MEMGRAPH_PASSWORD"),
            anthropic_api_key: env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            voyage_api_key: env::var("VOYAGE_API_KEY").unwrap_or_default(),
            tavily_api_key: env::var("TAVILY_API_KEY").unwrap_or_default(),
            apify_api_key: env::var("APIFY_API_KEY").unwrap_or_default(),
            web_host: env::var("WEB_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            web_port: env::var("WEB_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("WEB_PORT must be a number"),
            admin_username: env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string()),
            admin_password: required_env("ADMIN_PASSWORD"),
            city: env::var("CITY").unwrap_or_else(|_| "twincities".to_string()),
            city_name: None,
            city_lat: None,
            city_lng: None,
            city_radius_km: None,
            daily_budget_cents: 0,
            twilio_account_sid: env::var("TWILIO_ACCOUNT_SID").unwrap_or_default(),
            twilio_auth_token: env::var("TWILIO_AUTH_TOKEN").unwrap_or_default(),
            twilio_service_id: env::var("TWILIO_SERVICE_ID").unwrap_or_default(),
            admin_numbers,
        }
    }
}

impl Config {
    /// Log the first 8 characters of each sensitive env var for debugging.
    pub fn log_redacted(&self) {
        let vars = [
            ("MEMGRAPH_URI", &self.memgraph_uri),
            ("MEMGRAPH_USER", &self.memgraph_user),
            ("MEMGRAPH_PASSWORD", &self.memgraph_password),
            ("ANTHROPIC_API_KEY", &self.anthropic_api_key),
            ("VOYAGE_API_KEY", &self.voyage_api_key),
            ("TAVILY_API_KEY", &self.tavily_api_key),
            ("APIFY_API_KEY", &self.apify_api_key),
        ];
        for (name, value) in vars {
            if value.is_empty() {
                tracing::info!("{name} = (empty)");
            } else {
                tracing::info!("{name} = ({} chars)", value.len());
            }
        }
    }
}

fn required_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("{key} environment variable is required"))
}
