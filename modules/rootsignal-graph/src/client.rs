use neo4rs::{ConfigBuilder, Graph};

/// `GraphClient` is a type alias for `neo4rs::Graph`.
/// Previously this was a newtype wrapper; the alias preserves backward compatibility.
pub type GraphClient = Graph;

/// Connect to the graph database (Neo4j) with the given credentials.
pub async fn connect_graph(uri: &str, user: &str, password: &str) -> Result<Graph, neo4rs::Error> {
    let config = ConfigBuilder::default()
        .uri(uri)
        .user(user)
        .password(password)
        .db("neo4j")
        .fetch_size(500)
        .max_connections(10)
        .build()
        .unwrap();
    Graph::connect(config).await
}
