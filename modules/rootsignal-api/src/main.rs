use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
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
    pub admin_username: String,
    pub admin_password: String,
    pub city: String,
    pub rate_limiter: Mutex<HashMap<IpAddr, Vec<Instant>>>,
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
        GraphClient::connect(&config.neo4j_uri, &config.neo4j_user, &config.neo4j_password)
            .await?;

    let reader = Arc::new(PublicGraphReader::new(client.clone()));
    let schema = build_schema(reader.clone());

    let host = std::env::var("API_HOST").unwrap_or_else(|_| config.web_host.clone());
    let port = std::env::var("API_PORT").unwrap_or_else(|_| config.web_port.to_string());

    let state = Arc::new(AppState {
        schema,
        reader: PublicGraphReader::new(client.clone()),
        writer: GraphWriter::new(client),
        admin_username: config.admin_username,
        admin_password: config.admin_password,
        city: config.city.clone(),
        rate_limiter: Mutex::new(HashMap::new()),
    });

    let app = Router::new()
        // GraphQL
        .route("/graphql", get(graphiql).post(graphql_handler))
        // Health check
        .route("/", get(|| async { "ok" }))
        // Admin pages (Dioxus SSR)
        .route("/admin", get(pages::map_page))
        .route("/admin/nodes", get(pages::nodes_page))
        .route("/admin/nodes/{id}", get(pages::node_detail_page))
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
