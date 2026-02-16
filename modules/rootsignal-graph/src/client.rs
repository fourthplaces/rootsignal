use neo4rs::{ConfigBuilder, Graph};

/// Thin wrapper around neo4rs::Graph providing connection setup.
#[derive(Clone)]
pub struct GraphClient {
    pub(crate) graph: Graph,
}

impl GraphClient {
    /// Connect to Neo4j with the given credentials.
    pub async fn connect(uri: &str, user: &str, password: &str) -> Result<Self, neo4rs::Error> {
        let config = ConfigBuilder::default()
            .uri(uri)
            .user(user)
            .password(password)
            .fetch_size(500)
            .max_connections(10)
            .build()
            .unwrap();
        let graph = Graph::connect(config).await?;
        Ok(Self { graph })
    }

    /// Get a reference to the underlying neo4rs Graph.
    pub fn inner(&self) -> &Graph {
        &self.graph
    }
}
