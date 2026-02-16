pub mod auth;
pub mod clusters;
pub mod contacts;
pub mod context;
pub mod entities;
pub mod error;
pub mod findings;
pub mod heat_map;
pub mod hotspots;
pub mod investigations;
pub mod loaders;
pub mod locations;
pub mod notes;
pub mod observations;
pub mod schedules;
pub mod search;
pub mod service_areas;
pub mod signals;
pub mod sources;
pub mod stats;
pub mod tags;
pub mod workflows;

use std::sync::Arc;

use async_graphql::dataloader::DataLoader;
use async_graphql::*;
use rootsignal_core::ServerDeps;

use loaders::*;

/// Merged query root composing all domain query modules.
#[derive(MergedObject, Default)]
pub struct QueryRoot(
    entities::EntityQuery,
    tags::TagQuery,
    observations::ObservationQuery,
    investigations::InvestigationQuery,
    heat_map::HeatMapQuery,
    clusters::ClusterQuery,
    stats::StatsQuery,
    search::SearchQuery,
    workflows::WorkflowQuery,
    sources::SourceQuery,
    service_areas::ServiceAreaQuery,
    signals::SignalQuery,
    findings::FindingQuery,
);

/// Merged mutation root composing all domain mutation modules.
#[derive(MergedObject, Default)]
pub struct MutationRoot(
    auth::AuthMutation,
    entities::mutations::EntityMutation,
    observations::mutations::ObservationMutation,
    workflows::WorkflowMutation,
    sources::mutations::SourceMutation,
    service_areas::mutations::ServiceAreaMutation,
    signals::mutations::SignalMutation,
    findings::mutations::FindingMutation,
    heat_map::mutations::HeatMapMutation,
);

pub type AppSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

pub fn build_schema(deps: Arc<ServerDeps>) -> AppSchema {
    let pool = deps.pool().clone();

    // Build JWT service if configured
    let jwt_service = deps
        .config
        .jwt_secret
        .as_ref()
        .map(|secret| auth::jwt::JwtService::new(secret, "rootsignal".to_string()));

    let mut builder = Schema::build(
        QueryRoot::default(),
        MutationRoot::default(),
        EmptySubscription,
    )
    .data(pool.clone())
    .data(deps)
    // DataLoaders
    .data(DataLoader::new(
        EntityByIdLoader { pool: pool.clone() },
        tokio::spawn,
    ))
    .data(DataLoader::new(
        ServiceByIdLoader { pool: pool.clone() },
        tokio::spawn,
    ))
    .data(DataLoader::new(
        TagsForLoader { pool: pool.clone() },
        tokio::spawn,
    ))
    .data(DataLoader::new(
        LocationsForLoader { pool: pool.clone() },
        tokio::spawn,
    ))
    .data(DataLoader::new(
        SchedulesForLoader { pool: pool.clone() },
        tokio::spawn,
    ))
    .data(DataLoader::new(
        ContactsForLoader { pool: pool.clone() },
        tokio::spawn,
    ))
    .data(DataLoader::new(
        NotesForLoader { pool: pool.clone() },
        tokio::spawn,
    ))
    .data(DataLoader::new(
        TranslationLoader { pool: pool.clone() },
        tokio::spawn,
    ));

    // Register JWT service if configured
    if let Some(jwt) = jwt_service {
        builder = builder.data(jwt);
    }

    builder.limit_depth(10).limit_complexity(1000).finish()
}
