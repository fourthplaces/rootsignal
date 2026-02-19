//! Test utilities for spinning up a real Neo4j instance via testcontainers.

use testcontainers::{
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

use crate::GraphClient;

const TEST_PASSWORD: &str = "testpassword";

/// Connect to Neo4j for tests. Returns a handle (keep alive!) + connected GraphClient.
///
/// If `NEO4J_TEST_URI` is set (e.g. `bolt://127.0.0.1:7687`), connects to that instance
/// using `NEO4J_TEST_USER` (default `neo4j`) and `NEO4J_TEST_PASSWORD` (default `rootsignal`).
/// Otherwise, spins up a fresh Neo4j testcontainer.
pub async fn neo4j_container() -> (Box<dyn std::any::Any + Send>, GraphClient) {
    if let Ok(uri) = std::env::var("NEO4J_TEST_URI") {
        let user = std::env::var("NEO4J_TEST_USER").unwrap_or_else(|_| "neo4j".to_string());
        let password =
            std::env::var("NEO4J_TEST_PASSWORD").unwrap_or_else(|_| "rootsignal".to_string());
        let client = GraphClient::connect(&uri, &user, &password)
            .await
            .expect("Failed to connect to external Neo4j");
        // Return a unit value as the "container handle" — nothing to keep alive.
        (Box::new(()), client)
    } else {
        // Skip log-based waiting — Neo4j 5.x plugin installation writes to stdout then
        // the JVM logs go elsewhere, causing EndOfStream/timeout with log strategies.
        // Instead, start immediately and poll for Bolt connectivity.
        let image = GenericImage::new("neo4j", "5.25.1-enterprise")
            .with_exposed_port(ContainerPort::Tcp(7687))
            .with_wait_for(WaitFor::Nothing)
            .with_env_var("NEO4J_AUTH", format!("neo4j/{TEST_PASSWORD}"))
            .with_env_var("NEO4J_ACCEPT_LICENSE_AGREEMENT", "yes")
            .with_env_var("NEO4J_PLUGINS", "[\"graph-data-science\"]")
            .with_env_var("NEO4J_dbms_security_procedures_unrestricted", "gds.*");

        let container: ContainerAsync<GenericImage> = image
            .start()
            .await
            .expect("Failed to start Neo4j container");

        let host_port = container
            .get_host_port_ipv4(7687)
            .await
            .expect("Failed to get Neo4j host port");

        let uri = format!("bolt://127.0.0.1:{host_port}");

        // Poll until Neo4j accepts Bolt connections (up to 180s).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        let client = loop {
            match GraphClient::connect(&uri, "neo4j", TEST_PASSWORD).await {
                Ok(c) => break c,
                Err(_) if std::time::Instant::now() < deadline => {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(e) => panic!("Neo4j failed to become ready within 180s: {e}"),
            }
        };

        (Box::new(container), client)
    }
}

/// Backwards-compatible alias.
pub use neo4j_container as memgraph_container;
