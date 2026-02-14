pub mod contacts;
pub mod context;
pub mod entities;
pub mod error;
pub mod heat_map;
pub mod hotspots;
pub mod listings;
pub mod loaders;
pub mod locations;
pub mod notes;
pub mod observations;
pub mod schedules;
pub mod search;
pub mod sources;
pub mod stats;
pub mod tags;

use std::sync::Arc;

use async_graphql::dataloader::DataLoader;
use async_graphql::*;
use taproot_core::ServerDeps;

use loaders::*;

/// Merged query root composing all domain query modules.
#[derive(MergedObject, Default)]
pub struct QueryRoot(
    listings::ListingQuery,
    entities::EntityQuery,
    tags::TagQuery,
    observations::ObservationQuery,
    heat_map::HeatMapQuery,
    stats::StatsQuery,
    search::SearchQuery,
);

pub type AppSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

pub fn build_schema(deps: Arc<ServerDeps>) -> AppSchema {
    let pool = deps.pool().clone();
    Schema::build(QueryRoot::default(), EmptyMutation, EmptySubscription)
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
        // Limits
        .limit_depth(10)
        .limit_complexity(1000)
        .finish()
}
