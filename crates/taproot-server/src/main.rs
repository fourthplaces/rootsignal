use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

mod graphql;
mod routes;

// Import Restate traits to bring `.serve()` into scope
use taproot_domains::scraping::restate::{
    SchedulerService, ScrapeWorkflow, SourceObject,
};
use taproot_domains::extraction::restate::ExtractWorkflow;
use taproot_domains::investigations::restate::InvestigateWorkflow;
use taproot_domains::translation::restate::TranslateWorkflow;
use taproot_domains::clustering::ClusteringJob;
use taproot_domains::listings::restate::ListingsService;
use taproot_domains::entities::restate::tags::TagsService;

/// Wrapper to make OpenAi implement our dyn-compatible EmbeddingService trait.
struct OpenAiEmbeddingService {
    ai: Arc<ai_client::OpenAi>,
}

#[async_trait]
impl taproot_core::EmbeddingService for OpenAiEmbeddingService {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        use ai_client::EmbedAgent;
        self.ai.embed(text.to_string()).await
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        use ai_client::EmbedAgent;
        self.ai.embed_batch(texts.to_vec()).await
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    tracing::info!("Starting taproot-server");

    // Load config
    let config = taproot_core::AppConfig::from_env()?;
    let port = config.port;

    // Database pool
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    tracing::info!("Connected to database");

    // Run migrations
    sqlx::migrate!("../../migrations").run(&pool).await?;
    tracing::info!("Migrations complete");

    // HTTP client
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // AI clients
    let openai = Arc::new(ai_client::OpenAi::new(&config.openai_api_key, "gpt-4o"));
    let claude = config
        .anthropic_api_key
        .as_ref()
        .map(|key| Arc::new(ai_client::Claude::new(key, "claude-sonnet-4-5-20250929")));

    // Ingestor (default to Firecrawl if available, else HTTP)
    let ingestor: Arc<dyn taproot_core::Ingestor> = if config.firecrawl_api_key.is_some() {
        taproot_domains::scraping::adapters::build_ingestor(
            "firecrawl",
            &http_client,
            config.firecrawl_api_key.as_deref(),
        )?
    } else {
        taproot_domains::scraping::adapters::build_ingestor("http", &http_client, None)?
    };

    // Web searcher
    let web_searcher =
        taproot_domains::scraping::adapters::build_web_searcher(&config.tavily_api_key, &http_client);

    // Embedding service (OpenAI embeddings via wrapper)
    let embedding_service: Arc<dyn taproot_core::EmbeddingService> =
        Arc::new(OpenAiEmbeddingService {
            ai: openai.clone(),
        });

    // ServerDeps
    let server_deps = Arc::new(taproot_core::ServerDeps::new(
        pool.clone(),
        http_client,
        openai,
        claude,
        ingestor,
        web_searcher,
        embedding_service,
        config.clone(),
    ));

    // ─── Restate Endpoint ────────────────────────────────────────────────────

    let restate_endpoint = restate_sdk::endpoint::Endpoint::builder()
        .bind(
            taproot_domains::scraping::ScrapeWorkflowImpl::with_deps(server_deps.clone()).serve(),
        )
        .bind(
            taproot_domains::scraping::SourceObjectImpl::with_deps(server_deps.clone()).serve(),
        )
        .bind(
            taproot_domains::scraping::SchedulerServiceImpl::with_deps(server_deps.clone())
                .serve(),
        )
        .bind(
            taproot_domains::extraction::ExtractWorkflowImpl::with_deps(server_deps.clone())
                .serve(),
        )
        .bind(
            taproot_domains::investigations::InvestigateWorkflowImpl::with_deps(
                server_deps.clone(),
            )
            .serve(),
        )
        .bind(
            taproot_domains::translation::TranslateWorkflowImpl::with_deps(server_deps.clone())
                .serve(),
        )
        .bind(
            taproot_domains::listings::ListingsServiceImpl::with_deps(server_deps.clone())
                .serve(),
        )
        .bind(
            taproot_domains::entities::TagsServiceImpl::with_deps(server_deps.clone())
                .serve(),
        )
        .bind(
            taproot_domains::clustering::ClusteringJobImpl::with_deps(server_deps.clone())
                .serve(),
        )
        .build();

    // ─── Axum App (assessment routes) ────────────────────────────────────────

    let axum_app = routes::build_router(pool.clone(), &config.allowed_origins);

    // ─── Start servers ───────────────────────────────────────────────────────

    let restate_addr = format!("0.0.0.0:{}", port);
    let axum_addr = format!("0.0.0.0:{}", port + 1);

    tracing::info!(restate = %restate_addr, axum = %axum_addr, "Starting servers");

    // Auto-register with Restate admin
    if let Some(admin_url) = &server_deps.config.restate_admin_url {
        let self_url = server_deps
            .config
            .restate_self_url
            .clone()
            .unwrap_or_else(|| format!("http://localhost:{}", port));

        let client = reqwest::Client::new();
        let mut request = client
            .post(format!("{}/deployments", admin_url))
            .json(&serde_json::json!({
                "uri": self_url,
                "force": true,
            }));

        if let Some(token) = &server_deps.config.restate_auth_token {
            request = request.bearer_auth(token);
        }

        match request.send().await {
            Ok(resp) => {
                tracing::info!(status = %resp.status(), "Registered with Restate admin");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to register with Restate admin");
            }
        }
    }

    // Run both servers concurrently
    let restate_handle = tokio::spawn(async move {
        restate_sdk::http_server::HttpServer::new(restate_endpoint)
            .listen_and_serve(restate_addr.parse().unwrap())
            .await;
    });

    let axum_handle = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(&axum_addr).await.unwrap();
        tracing::info!("Axum assessment UI at http://{}", axum_addr);
        axum::serve(listener, axum_app).await.unwrap();
    });

    tokio::select! {
        _ = restate_handle => {},
        _ = axum_handle => {},
    }

    Ok(())
}
