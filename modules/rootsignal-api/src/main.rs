use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::Result;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::State,
    http::{header, HeaderValue},
    response::Html,
    routing::{get, post},
    Router,
};
use tokio::sync::Mutex;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::Config;
use rootsignal_graph::{GraphClient, GraphWriter, PublicGraphReader};
use twilio::TwilioService;

mod auth;
mod components;
mod graphql;
mod pages;
mod rest;
mod templates;

use graphql::{build_schema, ApiSchema};

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
}

async fn graphql_handler(
    State(state): State<Arc<AppState>>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    state.schema.execute(req.into_inner()).await.into()
}

async fn graphiql() -> Html<String> {
    Html(async_graphql::http::GraphiQLSource::build().endpoint("/graphql").finish())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("rootsignal=info".parse()?))
        .init();

    let config = Config::web_from_env();

    let client =
        GraphClient::connect(&config.memgraph_uri, &config.memgraph_user, &config.memgraph_password)
            .await?;

    rootsignal_graph::migrate::migrate(&client)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

    let reader = Arc::new(PublicGraphReader::new(client.clone()));
    let schema = build_schema(reader.clone());

    let host = std::env::var("API_HOST").unwrap_or_else(|_| config.web_host.clone());
    let port = std::env::var("API_PORT")
        .or_else(|_| std::env::var("PORT"))
        .unwrap_or_else(|_| config.web_port.to_string());

    // Clone client for the scout interval (before moving into AppState)
    let scout_client = client.clone();

    let twilio = if !config.twilio_account_sid.is_empty() {
        Some(TwilioService::new(twilio::TwilioOptions {
            account_sid: config.twilio_account_sid.clone(),
            auth_token: config.twilio_auth_token.clone(),
            service_id: config.twilio_service_id.clone(),
        }))
    } else {
        None
    };

    let state = Arc::new(AppState {
        schema,
        reader: PublicGraphReader::new(client.clone()),
        writer: GraphWriter::new(client.clone()),
        graph_client: client,
        config: config.clone(),
        twilio,
        city: config.city.clone(),
        rate_limiter: Mutex::new(HashMap::new()),
        scout_cancel: Arc::new(AtomicBool::new(false)),
    });

    let app = Router::new()
        // GraphQL
        .route("/graphql", get(graphiql).post(graphql_handler))
        // Health check
        .route("/", get(|| async { "ok" }))
        // Auth (no session required)
        .route("/admin/login", get(pages::login_page).post(pages::login_submit))
        .route("/admin/verify", post(pages::verify_submit))
        .route("/admin/logout", get(pages::logout))
        // Admin pages (session required via AdminSession extractor)
        .route("/admin", get(pages::map_page))
        .route("/admin/nodes", get(pages::nodes_page))
        .route("/admin/nodes/{id}", get(pages::node_detail_page))
        .route("/admin/stories", get(pages::stories_page))
        .route("/admin/stories/{id}", get(pages::story_detail_page))
        .route("/admin/cities", get(pages::cities_page).post(pages::create_city))
        .route("/admin/cities/{slug}", get(pages::city_detail_page))
        .route("/admin/cities/{slug}/scout", post(pages::run_city_scout))
        .route("/admin/cities/{slug}/scout/stop", post(pages::stop_city_scout))
        .route("/admin/cities/{slug}/scout/reset", post(pages::reset_scout_lock))
        .route("/admin/quality", get(pages::quality_dashboard))
        // REST API
        .route("/api/nodes/near", get(rest::api_nodes_near))
        .route("/api/stories", get(rest::api_stories))
        .route("/api/stories/{id}", get(rest::api_story_detail))
        .route("/api/stories/{id}/signals", get(rest::api_story_signals))
        .route("/api/stories/{id}/actors", get(rest::api_story_actors))
        .route("/api/stories/category/{category}", get(rest::api_stories_by_category))
        .route("/api/stories/arc/{arc}", get(rest::api_stories_by_arc))
        .route("/api/signals", get(rest::api_signals))
        .route("/api/signals/{id}", get(rest::api_signal_detail))
        .route("/api/actors", get(rest::api_actors))
        .route("/api/actors/{id}", get(rest::api_actor_detail))
        .route("/api/actors/{id}/stories", get(rest::api_actor_stories))
        .route("/api/tensions/{id}/responses", get(rest::api_tension_responses))
        .route("/api/editions", get(rest::api_editions))
        .route("/api/editions/latest", get(rest::api_edition_latest))
        .route("/api/editions/{id}", get(rest::api_edition_detail))
        .route("/api/submit", post(rest::submit::api_submit))
        // Scout
        .route("/api/scout/run", post(rest::scout::scout_run_handler))
        .route("/api/scout/status", get(rest::scout::scout_status))
        .with_state(state)
        // CORS
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        // Privacy headers: no caching, no tracking
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::PRAGMA,
            HeaderValue::from_static("no-cache"),
        ))
        // Logging layer: method + path + status + latency only (no query params, no IP)
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        path = %request.uri().path(),
                    )
                }),
        );

    // Start scout interval loop if configured
    let scout_interval: u64 = std::env::var("SCOUT_INTERVAL_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if scout_interval > 0
        && !config.anthropic_api_key.is_empty()
        && !config.voyage_api_key.is_empty()
        && !config.tavily_api_key.is_empty()
    {
        rest::scout::start_scout_interval(
            scout_client,
            config.clone(),
            scout_interval,
        );
    } else if scout_interval > 0 {
        info!("SCOUT_INTERVAL_HOURS={scout_interval} but API keys not set — scout interval disabled");
    }

    let addr = format!("{host}:{port}");
    info!("Root Signal API starting on {addr}");
    info!("GraphiQL IDE available at http://{addr}/graphql");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
