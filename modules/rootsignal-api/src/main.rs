use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use async_graphql::http::GraphiQLSource;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::State,
    http::{header, HeaderValue, Method},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tokio::sync::Mutex;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::Config;
use rootsignal_graph::{CacheStore, CachedReader, GraphClient, GraphWriter, PublicGraphReader};
use twilio::TwilioService;

mod db;
mod graphql;
mod jwt;
mod link_preview;
mod restate_client;

use graphql::context::AuthContext;
use graphql::mutations::{ClientIp, RateLimiter, ResponseHeaders};
use graphql::{build_schema, ApiSchema};
use jwt::JwtService;
use restate_client::RestateClient;

pub struct AppState {
    pub schema: ApiSchema,
    pub reader: PublicGraphReader,
    pub writer: GraphWriter,
    pub graph_client: GraphClient,
    pub config: Config,
    pub twilio: Option<TwilioService>,
    pub region: String,
    pub rate_limiter: Mutex<HashMap<IpAddr, Vec<Instant>>>,
    pub jwt_service: JwtService,
}

async fn graphql_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    req: GraphQLRequest,
) -> axum::response::Response {
    // Extract JWT from cookie
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let claims = jwt::parse_auth_cookie(cookie_header)
        .and_then(|token| state.jwt_service.verify_token(token).ok());

    // Build per-request context data
    let response_headers = Arc::new(ResponseHeaders(Mutex::new(Vec::new())));
    let auth_context = AuthContext(claims);
    let client_ip = ClientIp(addr.ip());

    let mut request = req.into_inner();
    request = request
        .data(auth_context)
        .data(client_ip)
        .data(response_headers.clone());

    let gql_response = state.schema.execute(request).await;

    // Build HTTP response with any headers set by mutations (e.g., Set-Cookie)
    let mut response: axum::response::Response =
        GraphQLResponse::from(gql_response).into_response();

    let extra_headers = response_headers.0.lock().await;
    for (name, value) in extra_headers.iter() {
        if let (Ok(name), Ok(value)) = (
            axum::http::header::HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            response.headers_mut().append(name, value);
        }
    }

    response
}

async fn graphiql() -> impl IntoResponse {
    if cfg!(debug_assertions) {
        Html(GraphiQLSource::build().endpoint("/graphql").finish()).into_response()
    } else {
        axum::http::StatusCode::NOT_FOUND.into_response()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("rootsignal=info".parse()?))
        .init();

    let config = Config::web_from_env();
    config.log_redacted();

    let client = GraphClient::connect(
        &config.neo4j_uri,
        &config.neo4j_user,
        &config.neo4j_password,
    )
    .await?;

    rootsignal_graph::migrate::migrate(&client)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

    // Build the in-memory cache. Block until loaded — no HTTP traffic until ready.
    info!("Loading signal cache from Neo4j…");
    let initial_cache = rootsignal_graph::cache::SignalCache::load(&client)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load initial cache: {e}"))?;
    let cache_store = Arc::new(CacheStore::new(initial_cache));

    // Spawn background reload loop
    cache_store.spawn_reload_loop(client.clone());

    let neo4j_reader = PublicGraphReader::new(client.clone());
    let reader = Arc::new(CachedReader::new(cache_store.clone(), neo4j_reader));
    let writer = Arc::new(GraphWriter::new(client.clone()));
    let jwt_service = JwtService::new(
        if config.session_secret.is_empty() {
            &config.admin_password
        } else {
            &config.session_secret
        },
        "rootsignal".to_string(),
    );

    let host = std::env::var("API_HOST").unwrap_or_else(|_| config.web_host.clone());
    let port = std::env::var("API_PORT")
        .or_else(|_| std::env::var("PORT"))
        .unwrap_or_else(|_| config.web_port.to_string());

    let twilio = if !config.twilio_account_sid.is_empty() {
        Some(Arc::new(TwilioService::new(twilio::TwilioOptions {
            account_sid: config.twilio_account_sid.clone(),
            auth_token: config.twilio_auth_token.clone(),
            service_id: config.twilio_service_id.clone(),
        })))
    } else {
        None
    };

    // ========== Postgres ==========
    // Connect to Postgres for the web archive, scout runs, and Restate workflows.
    let pg_pool = match std::env::var("DATABASE_URL") {
        Ok(database_url) => {
            match sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&database_url)
                .await
            {
                Ok(pool) => Some(pool),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to connect to Postgres — Restate workflows disabled");
                    None
                }
            }
        }
        Err(_) => {
            tracing::warn!("DATABASE_URL not set — Restate workflows disabled");
            None
        }
    };

    // Run SQL migrations if Postgres is available
    if let Some(ref pool) = pg_pool {
        sqlx::migrate!("./migrations")
            .run(pool)
            .await
            .map_err(|e| anyhow::anyhow!("Postgres migration failed: {e}"))?;
        info!("Postgres migrations applied");
    }

    let restate_client = std::env::var("RESTATE_INGRESS_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .map(RestateClient::new);
    if restate_client.is_some() {
        info!("Restate ingress configured — runScout will dispatch via Restate");
    }

    let schema = build_schema(
        reader.clone(),
        writer.clone(),
        jwt_service.clone(),
        Arc::new(config.clone()),
        twilio.clone(),
        RateLimiter(Mutex::new(HashMap::new())),
        Arc::new(client.clone()),
        cache_store.clone(),
        restate_client,
        pg_pool.clone(),
    );

    // ========== Restate endpoint ==========
    // Runs on a separate port alongside the Axum GraphQL server.
    // Workflows will be bound here as they are implemented (Phase 2+).
    let restate_port: u16 = std::env::var("RESTATE_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(9080);

    if let Some(ref pool) = pg_pool {
        let pool = pool.clone();
        let scout_deps = Arc::new(rootsignal_scout::workflows::ScoutDeps::from_config(
            client.clone(),
            pool,
            &config,
        ));

        let mut builder = restate_sdk::endpoint::Endpoint::builder();

        // Configure Restate request identity verification
        if let Ok(identity_key) = std::env::var("RESTATE_IDENTITY_KEY") {
            info!("Restate identity key configured");
            builder = builder
                .identity_key(&identity_key)
                .expect("Invalid Restate identity key");
        }

        use rootsignal_scout::workflows::bootstrap::{BootstrapWorkflow, BootstrapWorkflowImpl};
        use rootsignal_scout::workflows::actor_discovery::{ActorDiscoveryWorkflow, ActorDiscoveryWorkflowImpl};
        use rootsignal_scout::workflows::actor_discovery_batch::{ActorDiscoveryBatchWorkflow, ActorDiscoveryBatchWorkflowImpl};
        use rootsignal_scout::workflows::actor_service::{ActorService, ActorServiceImpl};
        use rootsignal_scout::workflows::scrape::{ScrapeWorkflow, ScrapeWorkflowImpl};
        use rootsignal_scout::workflows::synthesis::{SynthesisWorkflow, SynthesisWorkflowImpl};
        use rootsignal_scout::workflows::situation_weaver::{SituationWeaverWorkflow, SituationWeaverWorkflowImpl};
        use rootsignal_scout::workflows::supervisor::{SupervisorWorkflow, SupervisorWorkflowImpl};
        use rootsignal_scout::workflows::full_run::{FullScoutRunWorkflow, FullScoutRunWorkflowImpl};
        use rootsignal_scout::workflows::news_scanner::{NewsScanWorkflow, NewsScanWorkflowImpl};

        let endpoint = builder
            .bind(BootstrapWorkflowImpl::with_deps(scout_deps.clone()).serve())
            .bind(ActorDiscoveryWorkflowImpl::with_deps(scout_deps.clone()).serve())
            .bind(ActorDiscoveryBatchWorkflowImpl::with_deps(scout_deps.clone()).serve())
            .bind(ActorServiceImpl::with_deps(scout_deps.clone()).serve())
            .bind(ScrapeWorkflowImpl::with_deps(scout_deps.clone()).serve())
            .bind(SynthesisWorkflowImpl::with_deps(scout_deps.clone()).serve())
            .bind(SituationWeaverWorkflowImpl::with_deps(scout_deps.clone()).serve())
            .bind(SupervisorWorkflowImpl::with_deps(scout_deps.clone()).serve())
            .bind(FullScoutRunWorkflowImpl::with_deps(scout_deps.clone()).serve())
            .bind(NewsScanWorkflowImpl::with_deps(scout_deps.clone()).serve())
            .build();

        let restate_addr = format!("0.0.0.0:{restate_port}");
        info!("Restate endpoint starting on {restate_addr}");

        // Auto-register with Restate runtime if RESTATE_ADMIN_URL is set
        if let Ok(admin_url) = std::env::var("RESTATE_ADMIN_URL") {
            let self_url = std::env::var("RESTATE_SELF_URL")
                .unwrap_or_else(|_| format!("http://localhost:{restate_port}"));
            let auth_token = std::env::var("RESTATE_AUTH_TOKEN").ok();
            tokio::spawn(register_with_restate(admin_url, self_url, auth_token));
        }

        tokio::spawn(async move {
            let addr: std::net::SocketAddr = restate_addr.parse().expect("Invalid Restate address");
            let listener = loop {
                match tokio::net::TcpListener::bind(addr).await {
                    Ok(l) => break l,
                    Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                        tracing::warn!("Restate port {addr} in use, retrying in 3s");
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    }
                    Err(e) => {
                        tracing::error!("Restate endpoint failed to bind {addr}: {e}");
                        return;
                    }
                }
            };
            restate_sdk::http_server::HttpServer::new(endpoint)
                .serve(listener)
                .await;
        });
    }

    let state = Arc::new(AppState {
        schema,
        reader: PublicGraphReader::new(client.clone()),
        writer: GraphWriter::new(client.clone()),
        graph_client: client,
        config: config.clone(),
        twilio: twilio.map(|t| (*t).clone()),
        region: config.region.clone(),
        rate_limiter: Mutex::new(HashMap::new()),
        jwt_service: jwt_service.clone(),
    });

    let link_preview_cache = Arc::new(link_preview::LinkPreviewCache::new());

    let app = Router::new()
        // GraphQL
        .route("/graphql", get(graphiql).post(graphql_handler))
        // Health check
        .route("/", get(|| async { "ok" }))
        .with_state(state)
        // Link preview (separate state)
        .route(
            "/api/link-preview",
            get(link_preview::link_preview_handler).with_state(link_preview_cache),
        )
        // CORS: support credentials for JWT cookies
        .layer(if cfg!(debug_assertions) {
            tower_http::cors::CorsLayer::new()
                .allow_origin([
                    "http://localhost:5173".parse::<HeaderValue>().unwrap(),
                    "http://localhost:5174".parse::<HeaderValue>().unwrap(),
                ])
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([header::CONTENT_TYPE, header::COOKIE])
                .allow_credentials(true)
        } else {
            let origins: Vec<HeaderValue> = std::env::var("CORS_ORIGINS")
                .unwrap_or_else(|_| "https://rootsignal.app".to_string())
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            tower_http::cors::CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([header::CONTENT_TYPE, header::COOKIE])
                .allow_credentials(true)
        })
        // Security headers
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=63072000; includeSubDomains"),
        ))
        // Privacy headers: no caching for API responses
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::PRAGMA,
            HeaderValue::from_static("no-cache"),
        ))
        // Logging layer
        .layer(
            tower_http::trace::TraceLayer::new_for_http().make_span_with(
                |request: &axum::http::Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        path = %request.uri().path(),
                    )
                },
            ),
        );

    // ========== Axum HTTP server ==========
    let addr = format!("{host}:{port}");
    info!("Root Signal API starting on {addr}");
    info!("GraphQL endpoint at http://{addr}/graphql");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// Register this deployment with the Restate admin API after a brief delay.
async fn register_with_restate(admin_url: String, self_url: String, auth_token: Option<String>) {
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    info!(
        admin_url = %admin_url,
        self_url = %self_url,
        "Auto-registering with Restate"
    );
    let client = reqwest::Client::new();
    let mut request = client
        .post(format!("{}/deployments", admin_url))
        .json(&serde_json::json!({
            "uri": self_url,
            "force": true
        }));
    if let Some(token) = &auth_token {
        request = request.bearer_auth(token);
    }
    match request.send().await {
        Ok(resp) if resp.status().is_success() => {
            info!("Restate registration successful");
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!(status = %status, body = %body, "Restate registration failed");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to connect to Restate admin");
        }
    }
}
