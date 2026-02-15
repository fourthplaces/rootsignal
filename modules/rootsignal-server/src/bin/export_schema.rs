//! Export the GraphQL schema as SDL.
//!
//! Usage: cargo run --bin export-schema [output_path]

use async_graphql::*;
use rootsignal_server::graphql::{MutationRoot, QueryRoot};

fn main() {
    let schema = Schema::build(
        QueryRoot::default(),
        MutationRoot::default(),
        EmptySubscription,
    )
    .limit_depth(10)
    .limit_complexity(1000)
    .finish();

    let sdl = schema.sdl();

    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "modules/api-client-js/schema.graphql".to_string());

    std::fs::write(&out_path, &sdl).expect("Failed to write schema file");
    eprintln!("Schema exported to {out_path} ({} bytes)", sdl.len());
}
