//! Smoke test: connect to Neo4j via bolt://.
//! Run with: cargo test -p rootsignal-graph --test cloud_connect -- --ignored

use rootsignal_graph::{query, GraphClient};

#[tokio::test]
#[ignore] // requires live Neo4j credentials
async fn cloud_connect() {
    let uri = std::env::var("NEO4J_URI").expect("NEO4J_URI required");
    let user = std::env::var("NEO4J_USER").expect("NEO4J_USER required");
    let password = std::env::var("NEO4J_PASSWORD").expect("NEO4J_PASSWORD required");

    let client = GraphClient::connect(&uri, &user, &password)
        .await
        .expect("Failed to connect");

    let mut result = client
        .inner()
        .execute(query("RETURN 1 AS ping"))
        .await
        .unwrap();
    let row = result.next().await.unwrap().expect("No result row");
    let ping: i64 = row.get("ping").unwrap();
    assert_eq!(ping, 1);
}
