use std::env;

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    // Neo4j (bolt protocol via neo4rs driver)
    pub neo4j_uri: String,
    pub neo4j_user: String,
    pub neo4j_password: String,

    // AI providers
    pub anthropic_api_key: String,
    pub voyage_api_key: String,

    // Scraping
    pub serper_api_key: String,
    pub apify_api_key: String,

    // Web server
    pub web_host: String,
    pub web_port: u16,

    // Admin
    pub admin_username: String,
    pub admin_password: String,

    // Region
    pub region: String,

    // Region bootstrap (optional — for cold-start or explicit override)
    pub region_name: Option<String>,
    pub region_lat: Option<f64>,
    pub region_lng: Option<f64>,
    pub region_radius_km: Option<f64>,

    // Budget
    /// Daily budget limit in cents. 0 = unlimited.
    pub daily_budget_cents: u64,

    // Browserless (optional headless browser service)
    pub browserless_url: Option<String>,
    pub browserless_token: Option<String>,

    // Scout tuning
    /// Max web queries per scout run. Defaults to 50.
    pub max_web_queries_per_run: usize,

    // Data directory for run logs
    pub data_dir: std::path::PathBuf,

    // Twilio (for admin OTP auth)
    pub twilio_account_sid: String,
    pub twilio_auth_token: String,
    pub twilio_service_id: String,

    // Admin phone numbers (E.164) allowed to authenticate
    pub admin_numbers: Vec<String>,

    // Session signing secret (separate from admin_password)
    pub session_secret: String,
}

impl Config {
    /// Load configuration from environment variables.
    /// Panics with a clear message if required vars are missing.
    pub fn from_env() -> Self {
        Self {
            neo4j_uri: required_env("NEO4J_URI"),
            neo4j_user: required_env("NEO4J_USER"),
            neo4j_password: required_env("NEO4J_PASSWORD"),
            anthropic_api_key: required_env("ANTHROPIC_API_KEY"),
            voyage_api_key: required_env("VOYAGE_API_KEY"),
            serper_api_key: required_env("SERPER_API_KEY"),
            apify_api_key: env::var("APIFY_API_KEY").unwrap_or_default(),
            web_host: env::var("WEB_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            web_port: env::var("WEB_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("WEB_PORT must be a number"),
            admin_username: env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string()),
            admin_password: required_env("ADMIN_PASSWORD"),
            session_secret: String::new(),
            region: String::new(),
            region_name: None,
            region_lat: None,
            region_lng: None,
            region_radius_km: None,
            daily_budget_cents: 0,
            browserless_url: env::var("BROWSERLESS_URL").ok(),
            browserless_token: env::var("BROWSERLESS_TOKEN").ok(),
            max_web_queries_per_run: env::var("MAX_WEB_QUERIES_PER_RUN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
            data_dir: std::path::PathBuf::from(
                env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string()),
            ),
            twilio_account_sid: String::new(),
            twilio_auth_token: String::new(),
            twilio_service_id: String::new(),
            admin_numbers: Vec::new(),
        }
    }

    /// Load config for scout (no web server or admin fields needed).
    pub fn scout_from_env() -> Self {
        Self {
            neo4j_uri: required_env("NEO4J_URI"),
            neo4j_user: required_env("NEO4J_USER"),
            neo4j_password: required_env("NEO4J_PASSWORD"),
            anthropic_api_key: required_env("ANTHROPIC_API_KEY"),
            voyage_api_key: required_env("VOYAGE_API_KEY"),
            serper_api_key: required_env("SERPER_API_KEY"),
            apify_api_key: env::var("APIFY_API_KEY").unwrap_or_default(),
            web_host: String::new(),
            web_port: 0,
            admin_username: String::new(),
            admin_password: String::new(),
            session_secret: String::new(),
            region: env::var("REGION").or_else(|_| env::var("CITY")).unwrap_or_else(|_| "twincities".to_string()),
            region_name: env::var("REGION_NAME").or_else(|_| env::var("CITY_NAME")).ok(),
            region_lat: env::var("REGION_LAT").or_else(|_| env::var("CITY_LAT")).ok().and_then(|v| v.parse().ok()),
            region_lng: env::var("REGION_LNG").or_else(|_| env::var("CITY_LNG")).ok().and_then(|v| v.parse().ok()),
            region_radius_km: env::var("REGION_RADIUS_KM").or_else(|_| env::var("CITY_RADIUS_KM")).ok().and_then(|v| v.parse().ok()),
            daily_budget_cents: env::var("DAILY_BUDGET_CENTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            browserless_url: env::var("BROWSERLESS_URL").ok(),
            browserless_token: env::var("BROWSERLESS_TOKEN").ok(),
            max_web_queries_per_run: env::var("MAX_WEB_QUERIES_PER_RUN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
            data_dir: std::path::PathBuf::from(
                env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string()),
            ),
            twilio_account_sid: String::new(),
            twilio_auth_token: String::new(),
            twilio_service_id: String::new(),
            admin_numbers: Vec::new(),
        }
    }

    /// Load config for the scout supervisor (Neo4j + Anthropic + region + notifications).
    pub fn supervisor_from_env() -> Self {
        Self {
            neo4j_uri: required_env("NEO4J_URI"),
            neo4j_user: required_env("NEO4J_USER"),
            neo4j_password: required_env("NEO4J_PASSWORD"),
            anthropic_api_key: required_env("ANTHROPIC_API_KEY"),
            voyage_api_key: String::new(),
            serper_api_key: String::new(),
            apify_api_key: String::new(),
            web_host: String::new(),
            web_port: 0,
            admin_username: String::new(),
            admin_password: String::new(),
            session_secret: String::new(),
            region: env::var("REGION").or_else(|_| env::var("CITY")).unwrap_or_else(|_| "twincities".to_string()),
            region_name: None,
            region_lat: None,
            region_lng: None,
            region_radius_km: None,
            daily_budget_cents: env::var("SUPERVISOR_DAILY_BUDGET_CENTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),
            browserless_url: None,
            browserless_token: None,
            max_web_queries_per_run: 50,
            data_dir: std::path::PathBuf::from("data"),
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
            neo4j_uri: required_env("NEO4J_URI"),
            neo4j_user: required_env("NEO4J_USER"),
            neo4j_password: required_env("NEO4J_PASSWORD"),
            anthropic_api_key: env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            voyage_api_key: env::var("VOYAGE_API_KEY").unwrap_or_default(),
            serper_api_key: env::var("SERPER_API_KEY").unwrap_or_default(),
            apify_api_key: env::var("APIFY_API_KEY").unwrap_or_default(),
            web_host: env::var("WEB_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            web_port: env::var("WEB_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("WEB_PORT must be a number"),
            admin_username: env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string()),
            admin_password: required_env("ADMIN_PASSWORD"),
            session_secret: env::var("SESSION_SECRET").unwrap_or_default(),
            region: env::var("REGION").or_else(|_| env::var("CITY")).unwrap_or_else(|_| "twincities".to_string()),
            region_name: None,
            region_lat: None,
            region_lng: None,
            region_radius_km: None,
            daily_budget_cents: 0,
            browserless_url: env::var("BROWSERLESS_URL").ok(),
            browserless_token: env::var("BROWSERLESS_TOKEN").ok(),
            max_web_queries_per_run: 50,
            data_dir: std::path::PathBuf::from(
                env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string()),
            ),
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
            ("NEO4J_URI", &self.neo4j_uri),
            ("NEO4J_USER", &self.neo4j_user),
            ("NEO4J_PASSWORD", &self.neo4j_password),
            ("ANTHROPIC_API_KEY", &self.anthropic_api_key),
            ("VOYAGE_API_KEY", &self.voyage_api_key),
            ("SERPER_API_KEY", &self.serper_api_key),
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
