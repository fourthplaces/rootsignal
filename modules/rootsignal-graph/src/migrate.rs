use neo4rs::query;
use tracing::info;

use crate::GraphClient;

/// Run idempotent schema migrations: constraints, indexes.
pub async fn migrate(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Running schema migrations...");

    // --- UUID uniqueness constraints ---
    let constraints = [
        "CREATE CONSTRAINT event_id IF NOT EXISTS FOR (n:Event) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT give_id IF NOT EXISTS FOR (n:Give) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT ask_id IF NOT EXISTS FOR (n:Ask) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT tension_id IF NOT EXISTS FOR (n:Tension) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT evidence_id IF NOT EXISTS FOR (n:Evidence) REQUIRE n.id IS UNIQUE",
    ];

    for c in &constraints {
        g.run(query(c)).await?;
    }
    info!("UUID uniqueness constraints created");

    // --- NOT NULL constraints on safety-critical fields ---
    let not_null = [
        "CREATE CONSTRAINT event_sensitivity IF NOT EXISTS FOR (n:Event) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT give_sensitivity IF NOT EXISTS FOR (n:Give) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT ask_sensitivity IF NOT EXISTS FOR (n:Ask) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT tension_sensitivity IF NOT EXISTS FOR (n:Tension) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT event_confidence IF NOT EXISTS FOR (n:Event) REQUIRE n.confidence IS NOT NULL",
        "CREATE CONSTRAINT give_confidence IF NOT EXISTS FOR (n:Give) REQUIRE n.confidence IS NOT NULL",
        "CREATE CONSTRAINT ask_confidence IF NOT EXISTS FOR (n:Ask) REQUIRE n.confidence IS NOT NULL",
        "CREATE CONSTRAINT tension_confidence IF NOT EXISTS FOR (n:Tension) REQUIRE n.confidence IS NOT NULL",
    ];

    for c in &not_null {
        g.run(query(c)).await?;
    }
    info!("NOT NULL constraints on sensitivity/confidence created");

    // --- Spatial indexes (POINT) ---
    let spatial = [
        "CREATE POINT INDEX event_location IF NOT EXISTS FOR (n:Event) ON (n.location)",
        "CREATE POINT INDEX give_location IF NOT EXISTS FOR (n:Give) ON (n.location)",
        "CREATE POINT INDEX ask_location IF NOT EXISTS FOR (n:Ask) ON (n.location)",
        "CREATE POINT INDEX tension_location IF NOT EXISTS FOR (n:Tension) ON (n.location)",
    ];

    for s in &spatial {
        g.run(query(s)).await?;
    }
    info!("Spatial indexes created");

    // --- Full-text indexes ---
    let fulltext = [
        "CREATE FULLTEXT INDEX event_text IF NOT EXISTS FOR (n:Event) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX give_text IF NOT EXISTS FOR (n:Give) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX ask_text IF NOT EXISTS FOR (n:Ask) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX tension_text IF NOT EXISTS FOR (n:Tension) ON EACH [n.title, n.summary]",
    ];

    for f in &fulltext {
        g.run(query(f)).await?;
    }
    info!("Full-text indexes created");

    // --- Vector indexes (1024-dim for Voyage embeddings) ---
    // Neo4j 5.x vector index syntax
    let vector = [
        "CREATE VECTOR INDEX event_embedding IF NOT EXISTS FOR (n:Event) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX give_embedding IF NOT EXISTS FOR (n:Give) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX ask_embedding IF NOT EXISTS FOR (n:Ask) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX tension_embedding IF NOT EXISTS FOR (n:Tension) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
    ];

    for v in &vector {
        g.run(query(v)).await?;
    }
    info!("Vector indexes created");

    info!("Schema migration complete");
    Ok(())
}
