//! Test utilities for spinning up a real Memgraph instance via testcontainers.

use testcontainers::{
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage,
};

use crate::GraphClient;

/// Spin up a Memgraph container and return the container handle + connected GraphClient.
///
/// The container is dropped (and stopped) when `ContainerAsync` goes out of scope,
/// so callers must hold it alive for the duration of the test.
pub async fn memgraph_container() -> (ContainerAsync<GenericImage>, GraphClient) {
    let image = GenericImage::new("memgraph/memgraph-mage", "latest")
        .with_exposed_port(ContainerPort::Tcp(7687))
        .with_wait_for(WaitFor::message_on_stdout("You are running Memgraph"));

    let container = image
        .start()
        .await
        .expect("Failed to start Memgraph container");

    let host_port = container
        .get_host_port_ipv4(7687)
        .await
        .expect("Failed to get Memgraph host port");

    let uri = format!("bolt://127.0.0.1:{host_port}");
    let client = GraphClient::connect(&uri, "", "")
        .await
        .expect("Failed to connect to Memgraph");

    (container, client)
}
