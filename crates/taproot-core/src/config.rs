use anyhow::Result;

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct AppConfig {
    // Database
    pub database_url: String,

    // Server
    pub port: u16,

    // AI / LLM
    pub openai_api_key: String,
    pub anthropic_api_key: Option<String>,

    // Scraping
    pub tavily_api_key: String,
    pub firecrawl_api_key: Option<String>,
    pub apify_api_key: Option<String>,
    pub eventbrite_api_key: Option<String>,

    // Geocoding
    pub geocoding_api_key: Option<String>,

    // Restate
    pub restate_admin_url: Option<String>,
    pub restate_self_url: Option<String>,
    pub restate_auth_token: Option<String>,

    // Clustering
    pub cluster_similarity_threshold: f64,
    pub cluster_match_score_threshold: f64,
    pub cluster_merge_coherence_threshold: f64,
    pub cluster_geo_radius_meters: f64,
    pub cluster_time_window_hours: i64,
    pub cluster_batch_size: i64,
    pub hnsw_ef_search: i32,

    // CORS
    pub allowed_origins: Vec<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            database_url: std::env::var("DATABASE_URL")?,
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "9080".to_string())
                .parse()?,
            openai_api_key: std::env::var("OPENAI_API_KEY")?,
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            tavily_api_key: std::env::var("TAVILY_API_KEY")?,
            firecrawl_api_key: std::env::var("FIRECRAWL_API_KEY").ok(),
            apify_api_key: std::env::var("APIFY_API_KEY").ok(),
            eventbrite_api_key: std::env::var("EVENTBRITE_API_KEY").ok(),
            geocoding_api_key: std::env::var("GEOCODING_API_KEY").ok(),
            restate_admin_url: std::env::var("RESTATE_ADMIN_URL").ok(),
            restate_self_url: std::env::var("RESTATE_SELF_URL").ok(),
            restate_auth_token: std::env::var("RESTATE_AUTH_TOKEN").ok(),
            cluster_similarity_threshold: std::env::var("CLUSTER_SIMILARITY_THRESHOLD")
                .unwrap_or_else(|_| "0.92".to_string())
                .parse()
                .unwrap_or(0.92),
            cluster_match_score_threshold: std::env::var("CLUSTER_MATCH_SCORE_THRESHOLD")
                .unwrap_or_else(|_| "0.75".to_string())
                .parse()
                .unwrap_or(0.75),
            cluster_merge_coherence_threshold: std::env::var("CLUSTER_MERGE_COHERENCE_THRESHOLD")
                .unwrap_or_else(|_| "0.85".to_string())
                .parse()
                .unwrap_or(0.85),
            cluster_geo_radius_meters: std::env::var("CLUSTER_GEO_RADIUS_METERS")
                .unwrap_or_else(|_| "500.0".to_string())
                .parse()
                .unwrap_or(500.0),
            cluster_time_window_hours: std::env::var("CLUSTER_TIME_WINDOW_HOURS")
                .unwrap_or_else(|_| "24".to_string())
                .parse()
                .unwrap_or(24),
            cluster_batch_size: std::env::var("CLUSTER_BATCH_SIZE")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .unwrap_or(100),
            hnsw_ef_search: std::env::var("HNSW_EF_SEARCH")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .unwrap_or(100),
            allowed_origins: std::env::var("ALLOWED_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string())
                .collect(),
        })
    }
}
