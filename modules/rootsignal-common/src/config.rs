use std::env;

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    // Neo4j
    pub neo4j_uri: String,
    pub neo4j_user: String,
    pub neo4j_password: String,

    // AI providers
    pub anthropic_api_key: String,
    pub voyage_api_key: String,

    // Scraping
    pub firecrawl_api_key: String,
    pub tavily_api_key: String,

    // Web server
    pub web_host: String,
    pub web_port: u16,

    // Admin
    pub admin_username: String,
    pub admin_password: String,
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
            firecrawl_api_key: env::var("FIRECRAWL_API_KEY").unwrap_or_default(),
            tavily_api_key: required_env("TAVILY_API_KEY"),
            web_host: env::var("WEB_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            web_port: env::var("WEB_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("WEB_PORT must be a number"),
            admin_username: env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string()),
            admin_password: required_env("ADMIN_PASSWORD"),
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
            firecrawl_api_key: env::var("FIRECRAWL_API_KEY").unwrap_or_default(),
            tavily_api_key: required_env("TAVILY_API_KEY"),
            web_host: String::new(),
            web_port: 0,
            admin_username: String::new(),
            admin_password: String::new(),
        }
    }

    /// Load a minimal config for the web server (read-only, no AI keys needed).
    pub fn web_from_env() -> Self {
        Self {
            neo4j_uri: required_env("NEO4J_URI"),
            neo4j_user: required_env("NEO4J_USER"),
            neo4j_password: required_env("NEO4J_PASSWORD"),
            anthropic_api_key: String::new(),
            voyage_api_key: String::new(),
            firecrawl_api_key: String::new(),
            tavily_api_key: String::new(),
            web_host: env::var("WEB_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            web_port: env::var("WEB_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("WEB_PORT must be a number"),
            admin_username: env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string()),
            admin_password: required_env("ADMIN_PASSWORD"),
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
            ("FIRECRAWL_API_KEY", &self.firecrawl_api_key),
            ("TAVILY_API_KEY", &self.tavily_api_key),
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
