use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use async_graphql::http::GraphiQLSource;
use async_graphql_axum::{GraphQLProtocol, GraphQLRequest, GraphQLResponse, GraphQLWebSocket};
use axum::{
    extract::State,
    http::{header, HeaderValue, Method},
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use tokio::sync::Mutex;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;
use rootsignal_common::Config;
use rootsignal_graph::{connect_graph, CacheStore, CachedReader, GraphClient, GraphStore, PublicGraphReader};
use twilio::TwilioService;
use graphql::context::AuthContext;
use graphql::mutations::{ClientIp, RateLimiter, ResponseHeaders};
use graphql::{build_schema, ApiSchema};
use jwt::JwtService;
use scout_runner::ScoutRunner;


mod db;
mod debug_context;
mod event_broadcast;
mod event_cache;
mod graphql;
mod investigate;
mod investigate_tools;
mod jwt;
mod link_preview;
mod scout_runner;


pub struct AppState {
    pub schema: ApiSchema,
    pub reader: Arc<PublicGraphReader>,
    pub writer: GraphStore,
    pub graph_client: GraphClient,
    pub config: Config,
    pub twilio: Option<TwilioService>,
    pub region: String,
    pub rate_limiter: Mutex<HashMap<IpAddr, Vec<Instant>>>,
    pub jwt_service: JwtService,
    pub pg_pool: Option<sqlx::PgPool>,
    pub event_broadcast: Option<event_broadcast::EventBroadcast>,
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

async fn graphql_ws_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    protocol: GraphQLProtocol,
    ws: axum::extract::WebSocketUpgrade,
) -> axum::response::Response {
    // Extract JWT from cookie at WS upgrade time (primary auth gate)
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let claims = jwt::parse_auth_cookie(cookie_header)
        .and_then(|token| state.jwt_service.verify_token(token).ok());

    let auth_context = AuthContext(claims);

    let schema = state.schema.clone();

    ws.protocols(["graphql-transport-ws", "graphql-ws"])
        .on_upgrade(move |stream| {
            GraphQLWebSocket::new(stream, schema, protocol)
                .on_connection_init(move |_params| async move {
                    let mut data = async_graphql::Data::default();
                    data.insert(auth_context);
                    Ok(data)
                })
                .serve()
        })
}

async fn graphiql() -> impl IntoResponse {
    if cfg!(debug_assertions) {
        Html(
            GraphiQLSource::build()
                .endpoint("/graphql")
                .subscription_endpoint("/graphql/ws")
                .finish(),
        )
        .into_response()
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

    let client = connect_graph(
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
    let writer = Arc::new(GraphStore::new(client.clone()));
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
    let pg_pool = match std::env::var("DATABASE_URL") {
        Ok(database_url) => {
            match sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&database_url)
                .await
            {
                Ok(pool) => Some(pool),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to connect to Postgres — scout workflows disabled");
                    None
                }
            }
        }
        Err(_) => {
            tracing::warn!("DATABASE_URL not set — scout workflows disabled");
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

    // Spawn live event broadcast (PgListener → broadcast channel)
    let event_broadcast = pg_pool
        .as_ref()
        .map(|pool| event_broadcast::EventBroadcast::spawn(pool.clone()));

    // Hydrate in-memory event cache (most recent 500K events)
    let event_cache = if let Some(ref pool) = pg_pool {
        match event_cache::EventCache::hydrate(pool, 500_000).await {
            Ok(cache) => {
                let shared = std::sync::Arc::new(tokio::sync::RwLock::new(cache));
                // Spawn live update listener
                if let Some(ref broadcast) = event_broadcast {
                    event_cache::spawn_cache_listener(shared.clone(), broadcast);
                }
                Some(shared)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to hydrate event cache — admin queries will use Postgres directly");
                None
            }
        }
    } else {
        None
    };

    let store_factory = pg_pool
        .clone()
        .map(|_pool| rootsignal_scout::store::SignalReaderFactory::new(client.clone()));
    let engine_factory = pg_pool
        .clone()
        .map(|pool| rootsignal_scout::store::EngineFactory::new(client.clone(), pool));

    if store_factory.is_none() {
        tracing::warn!(
            "SignalReaderFactory not available — mutations that write signals will fail"
        );
    }

    // Build ScoutRunner for spawning seesaw engines directly
    let scout_runner = pg_pool.as_ref().map(|pool| {
        let scout_deps = Arc::new(rootsignal_scout::workflows::ScoutDeps::from_config(
            client.clone(),
            pool.clone(),
            &config,
        ));
        info!("ScoutRunner configured — runScout will spawn seesaw engines directly");
        let runner = ScoutRunner::new(scout_deps);
        // Resume any runs that were in-flight when the server last crashed
        let resume_runner = runner.clone();
        tokio::spawn(async move {
            resume_runner.resume_incomplete_runs().await;
        });
        // Background loop: process due scheduled scrapes every 15 minutes
        runner.clone().start_scheduled_scrapes_loop(
            GraphStore::new(client.clone()),
        );
        runner
    });

    let schema = build_schema(
        reader.clone(),
        writer.clone(),
        store_factory,
        engine_factory,
        jwt_service.clone(),
        Arc::new(config.clone()),
        twilio.clone(),
        RateLimiter(Mutex::new(HashMap::new())),
        Arc::new(client.clone()),
        cache_store.clone(),
        scout_runner.clone(),
        pg_pool.clone(),
        event_broadcast.clone(),
        event_cache.clone(),
    );

    let state = Arc::new(AppState {
        schema,
        reader: Arc::new(PublicGraphReader::new(client.clone())),
        writer: GraphStore::new(client.clone()),
        graph_client: client,
        config: config.clone(),
        twilio: twilio.map(|t| (*t).clone()),
        region: config.region.clone(),
        rate_limiter: Mutex::new(HashMap::new()),
        jwt_service: jwt_service.clone(),
        pg_pool: pg_pool.clone(),
        event_broadcast: event_broadcast.clone(),
    });

    let link_preview_cache = Arc::new(link_preview::LinkPreviewCache::new());

    let app = Router::new()
        // GraphQL
        .route("/graphql", get(graphiql).post(graphql_handler))
        // GraphQL WebSocket subscriptions
        .route(
            "/graphql/ws",
            get(graphql_ws_handler),
        )
        // AI investigation (SSE streaming)
        .route("/api/investigate", post(investigate::investigate_handler))
        // Debug context (markdown dump for Claude Code)
        .route("/api/debug-context", get(debug_context::debug_context_handler))
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

    let listener = loop {
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => break l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                tracing::warn!("API port {addr} in use, retrying in 3s");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
            Err(e) => return Err(e.into()),
        }
    };
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}


