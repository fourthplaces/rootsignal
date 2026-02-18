//! Smoke test: connect to Memgraph Cloud via bolt+ssc://.
//! Run with: cargo test -p rootsignal-graph --test cloud_connect -- --ignored

use rootsignal_graph::{query, GraphClient};

#[tokio::test]
#[ignore] // requires live Memgraph Cloud credentials
async fn cloud_connect() {
    let uri = std::env::var("MEMGRAPH_URI").expect("MEMGRAPH_URI required");
    let user = std::env::var("MEMGRAPH_USER").expect("MEMGRAPH_USER required");
    let password = std::env::var("MEMGRAPH_PASSWORD").expect("MEMGRAPH_PASSWORD required");

    let client = GraphClient::connect(&uri, &user, &password)
        .await
        .expect("Failed to connect");

    let mut result = client.inner().execute(query("RETURN 1 AS ping")).await.unwrap();
    let row = result.next().await.unwrap().expect("No result row");
    let ping: i64 = row.get("ping").unwrap();
    assert_eq!(ping, 1);
}
