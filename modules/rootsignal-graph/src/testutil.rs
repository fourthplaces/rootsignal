//! Test utilities for spinning up a real Neo4j instance via testcontainers.

use testcontainers::{
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

use crate::GraphClient;

/// Spin up a Neo4j container and return the container handle + connected GraphClient.
///
/// The container is dropped (and stopped) when `ContainerAsync` goes out of scope,
/// so callers must hold it alive for the duration of the test.
pub async fn neo4j_container() -> (ContainerAsync<GenericImage>, GraphClient) {
    let image = GenericImage::new("neo4j", "5.25.1-enterprise")
        .with_exposed_port(ContainerPort::Tcp(7687))
        .with_wait_for(WaitFor::message_on_stdout("Started."))
        .with_env_var("NEO4J_AUTH", "neo4j/test")
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
    let client = GraphClient::connect(&uri, "neo4j", "test")
        .await
        .expect("Failed to connect to Neo4j");

    (container, client)
}

/// Backwards-compatible alias.
pub use neo4j_container as memgraph_container;
