use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::AtomicBool;
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

mod graphql;
mod jwt;

use graphql::context::AuthContext;
use graphql::mutations::{ClientIp, RateLimiter, ResponseHeaders};
use graphql::{build_schema, ApiSchema};
use jwt::JwtService;

pub struct AppState {
    pub schema: ApiSchema,
    pub reader: PublicGraphReader,
    pub writer: GraphWriter,
    pub graph_client: GraphClient,
    pub config: Config,
    pub twilio: Option<TwilioService>,
    pub city: String,
    pub rate_limiter: Mutex<HashMap<IpAddr, Vec<Instant>>>,
    pub scout_cancel: Arc<AtomicBool>,
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

    let scout_cancel = Arc::new(AtomicBool::new(false));

    let twilio = if !config.twilio_account_sid.is_empty() {
        Some(Arc::new(TwilioService::new(twilio::TwilioOptions {
            account_sid: config.twilio_account_sid.clone(),
            auth_token: config.twilio_auth_token.clone(),
            service_id: config.twilio_service_id.clone(),
        })))
    } else {
        None
    };

    let schema = build_schema(
        reader.clone(),
        writer.clone(),
        jwt_service.clone(),
        Arc::new(config.clone()),
        twilio.clone(),
        RateLimiter(Mutex::new(HashMap::new())),
        scout_cancel.clone(),
        Arc::new(client.clone()),
        cache_store.clone(),
    );

    // Start scout interval loop if configured (before client is moved into AppState)
    let scout_interval: u64 = std::env::var("SCOUT_INTERVAL_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if scout_interval > 0
        && !config.anthropic_api_key.is_empty()
        && !config.voyage_api_key.is_empty()
        && !config.serper_api_key.is_empty()
    {
        graphql::mutations::start_scout_interval(client.clone(), config.clone(), scout_interval, cache_store.clone());
    } else if scout_interval > 0 {
        info!(
            "SCOUT_INTERVAL_HOURS={scout_interval} but API keys not set — scout interval disabled"
        );
    }

    let state = Arc::new(AppState {
        schema,
        reader: PublicGraphReader::new(client.clone()),
        writer: GraphWriter::new(client.clone()),
        graph_client: client,
        config: config.clone(),
        twilio: twilio.map(|t| (*t).clone()),
        city: config.city.clone(),
        rate_limiter: Mutex::new(HashMap::new()),
        scout_cancel: scout_cancel.clone(),
        jwt_service: jwt_service.clone(),
    });

    let app = Router::new()
        // GraphQL
        .route("/graphql", get(graphiql).post(graphql_handler))
        // Health check
        .route("/", get(|| async { "ok" }))
        .with_state(state)
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
