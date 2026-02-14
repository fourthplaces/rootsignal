use anyhow::{Context, Result};
use async_trait::async_trait;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

use rootsignal_server::routes;

// Import Restate traits to bring `.serve()` into scope
use rootsignal_domains::scraping::restate::{
    SchedulerService, ScrapeWorkflow, SourceObject,
};
use rootsignal_domains::extraction::restate::ExtractWorkflow;
use rootsignal_domains::investigations::restate::InvestigateWorkflow;
use rootsignal_domains::translation::restate::TranslateWorkflow;
use rootsignal_domains::clustering::ClusteringJob;
use rootsignal_domains::listings::restate::ListingsService;
use rootsignal_domains::entities::restate::tags::TagsService;

#[derive(Parser)]
#[command(name = "rootsignal-server", about = "Root Signal community signal server")]
struct Cli {
    /// Path to config TOML file
    #[arg(long, default_value = "./config/rootsignal.toml")]
    config: PathBuf,
}

/// Wrapper to make OpenAi implement our dyn-compatible EmbeddingService trait.
struct OpenAiEmbeddingService {
    ai: Arc<ai_client::OpenAi>,
}

#[async_trait]
impl rootsignal_core::EmbeddingService for OpenAiEmbeddingService {
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

    tracing::info!("Starting rootsignal-server");

    // Parse CLI args
    let cli = Cli::parse();

    // Load TOML config
    let config_path = cli.config.canonicalize().with_context(|| {
        format!(
            "Config file not found: {}. Create one or specify --config <path>",
            cli.config.display()
        )
    })?;
    let config_dir = config_path
        .parent()
        .expect("config file must have a parent directory");

    tracing::info!(config = %config_path.display(), "Loading config");

    let file_config = rootsignal_core::file_config::load_config(&config_path)?;
    let toml_value = rootsignal_core::file_config::load_toml_value(&config_path)?;

    // Load and resolve prompt templates
    let prompts = rootsignal_core::PromptRegistry::load(&file_config, config_dir, &toml_value)?;
    tracing::info!("Prompt templates loaded and validated");

    let file_config = Arc::new(file_config);
    let prompts = Arc::new(prompts);
    let port = file_config.server.port;

    // Load secrets from env vars
    let config = rootsignal_core::AppConfig::from_env()?;

    // Separate connection pools for HTTP (GraphQL/REST) and background workers
    let http_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;

    let worker_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&config.database_url)
        .await?;

    // Use http_pool as the primary pool for migrations and shared access
    let pool = http_pool.clone();

    tracing::info!("Connected to database (http_pool=20, worker_pool=8)");

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
    let ingestor: Arc<dyn rootsignal_core::Ingestor> = if config.firecrawl_api_key.is_some() {
        rootsignal_domains::scraping::adapters::build_ingestor(
            "firecrawl",
            &http_client,
            config.firecrawl_api_key.as_deref(),
        )?
    } else {
        rootsignal_domains::scraping::adapters::build_ingestor("http", &http_client, None)?
    };

    // Web searcher
    let web_searcher =
        rootsignal_domains::scraping::adapters::build_web_searcher(&config.tavily_api_key, &http_client);

    // Embedding service (OpenAI embeddings via wrapper)
    let embedding_service: Arc<dyn rootsignal_core::EmbeddingService> =
        Arc::new(OpenAiEmbeddingService {
            ai: openai.clone(),
        });

    // ServerDeps — HTTP handlers use http_pool, workers use worker_pool
    let http_deps = Arc::new(rootsignal_core::ServerDeps::new(
        http_pool.clone(),
        http_client.clone(),
        openai.clone(),
        claude.clone(),
        ingestor.clone(),
        web_searcher.clone(),
        embedding_service.clone(),
        config.clone(),
        file_config.clone(),
        prompts.clone(),
    ));

    let worker_deps = Arc::new(rootsignal_core::ServerDeps::new(
        worker_pool,
        http_client,
        openai,
        claude,
        ingestor,
        web_searcher,
        embedding_service,
        config.clone(),
        file_config.clone(),
        prompts.clone(),
    ));

    // Alias for backwards compat in registration logic
    let server_deps = http_deps.clone();

    // ─── Restate Endpoint ────────────────────────────────────────────────────

    let restate_endpoint = restate_sdk::endpoint::Endpoint::builder()
        .bind(
            rootsignal_domains::scraping::ScrapeWorkflowImpl::with_deps(worker_deps.clone()).serve(),
        )
        .bind(
            rootsignal_domains::scraping::SourceObjectImpl::with_deps(worker_deps.clone()).serve(),
        )
        .bind(
            rootsignal_domains::scraping::SchedulerServiceImpl::with_deps(worker_deps.clone())
                .serve(),
        )
        .bind(
            rootsignal_domains::extraction::ExtractWorkflowImpl::with_deps(worker_deps.clone())
                .serve(),
        )
        .bind(
            rootsignal_domains::investigations::InvestigateWorkflowImpl::with_deps(
                worker_deps.clone(),
            )
            .serve(),
        )
        .bind(
            rootsignal_domains::translation::TranslateWorkflowImpl::with_deps(worker_deps.clone())
                .serve(),
        )
        .bind(
            rootsignal_domains::listings::ListingsServiceImpl::with_deps(worker_deps.clone())
                .serve(),
        )
        .bind(
            rootsignal_domains::entities::TagsServiceImpl::with_deps(worker_deps.clone())
                .serve(),
        )
        .bind(
            rootsignal_domains::clustering::ClusteringJobImpl::with_deps(worker_deps.clone())
                .serve(),
        )
        .build();

    // ─── Axum App (assessment routes) ────────────────────────────────────────

    let axum_app = routes::build_router(http_deps.clone());

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
