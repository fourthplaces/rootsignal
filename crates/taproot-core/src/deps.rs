use crate::config::AppConfig;
use crate::ingestor::{Ingestor, WebSearcher};
use ai_client::OpenAi;
use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

/// Dyn-compatible embedding trait (wraps ai_client::EmbedAgent).
#[async_trait]
pub trait EmbeddingService: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// Central dependency container passed to all handlers and workflows.
#[derive(Clone)]
pub struct ServerDeps {
    pub db_pool: PgPool,
    pub http_client: reqwest::Client,
    pub ai: Arc<OpenAi>,
    pub claude: Option<Arc<ai_client::Claude>>,
    pub ingestor: Arc<dyn Ingestor>,
    pub web_searcher: Arc<dyn WebSearcher>,
    pub embedding_service: Arc<dyn EmbeddingService>,
    pub config: AppConfig,
}

impl ServerDeps {
    pub fn new(
        db_pool: PgPool,
        http_client: reqwest::Client,
        ai: Arc<OpenAi>,
        claude: Option<Arc<ai_client::Claude>>,
        ingestor: Arc<dyn Ingestor>,
        web_searcher: Arc<dyn WebSearcher>,
        embedding_service: Arc<dyn EmbeddingService>,
        config: AppConfig,
    ) -> Self {
        Self {
            db_pool,
            http_client,
            ai,
            claude,
            ingestor,
            web_searcher,
            embedding_service,
            config,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.db_pool
    }
}
