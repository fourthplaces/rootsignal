use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::events::{Event, Location};
use rootsignal_common::{GeoPoint, GeoPrecision};
use rootsignal_graph::{query, GraphClient, GraphProjector};
use causal::types::PersistedEvent;

pub fn stored(seq: i64, event: &Event) -> PersistedEvent {
    PersistedEvent {
        position: seq as u64,
        event_id: Uuid::new_v4(),
        parent_id: None,
        correlation_id: Uuid::new_v4(),
        event_type: event.event_type().to_string(),
        payload: serde_json::to_value(event).expect("serialize event"),
        created_at: Utc::now(),
        aggregate_type: None,
        aggregate_id: None,
        version: None,
        metadata: {
            let mut m = serde_json::Map::new();
            m.insert("run_id".into(), serde_json::json!("test"));
            m.insert("schema_v".into(), serde_json::json!(1));
            m
        },
        ephemeral: None,
        persistent: true,
    }
}

pub fn mpls() -> Location {
    loc("Minneapolis", 44.9778, -93.2650)
}

pub fn loc(name: &str, lat: f64, lng: f64) -> Location {
    Location {
        point: Some(GeoPoint {
            lat,
            lng,
            precision: GeoPrecision::Exact,
        }),
        name: Some(name.into()),
        address: None,
        role: None,
    }
}

pub fn loc_with_role(name: &str, lat: f64, lng: f64, role: &str) -> Location {
    Location {
        point: Some(GeoPoint {
            lat,
            lng,
            precision: GeoPrecision::Exact,
        }),
        name: Some(name.into()),
        address: None,
        role: Some(role.into()),
    }
}

pub async fn read_prop<T: for<'a> serde::Deserialize<'a> + Default>(
    client: &GraphClient,
    label: &str,
    id: Uuid,
    prop: &str,
) -> T {
    let cypher = format!("MATCH (n:{label} {{id: $id}}) RETURN n.{prop} AS val");
    let q = query(&cypher).param("id", id.to_string());
    let mut stream = client.execute(q).await.expect("query failed");
    if let Some(row) = stream.next().await.expect("stream failed") {
        row.get::<T>("val").unwrap_or_default()
    } else {
        T::default()
    }
}

pub async fn count_edges(
    client: &GraphClient,
    from_id: Uuid,
    edge_type: &str,
    to_label: &str,
) -> i64 {
    let cypher = format!(
        "MATCH (n {{id: $id}})-[:{edge_type}]->(m:{to_label}) RETURN count(m) AS cnt"
    );
    let q = query(&cypher).param("id", from_id.to_string());
    let mut stream = client.execute(q).await.expect("query failed");
    stream
        .next()
        .await
        .expect("stream failed")
        .map(|r| r.get::<i64>("cnt").unwrap_or(0))
        .unwrap_or(0)
}

pub async fn count_nodes(client: &GraphClient, label: &str) -> i64 {
    let cypher = format!("MATCH (n:{label}) RETURN count(n) AS cnt");
    let q = query(&cypher);
    let mut stream = client.execute(q).await.expect("query failed");
    stream
        .next()
        .await
        .expect("stream failed")
        .map(|r| r.get::<i64>("cnt").unwrap_or(0))
        .unwrap_or(0)
}

pub async fn read_source_prop<T: for<'a> serde::Deserialize<'a> + Default>(
    client: &GraphClient,
    canonical_key: &str,
    prop: &str,
) -> T {
    let cypher = format!("MATCH (s:Source {{canonical_key: $key}}) RETURN s.{prop} AS val");
    let q = query(&cypher).param("key", canonical_key);
    let mut stream = client.execute(q).await.expect("query failed");
    if let Some(row) = stream.next().await.expect("stream failed") {
        row.get::<T>("val").unwrap_or_default()
    } else {
        T::default()
    }
}

pub async fn project_all(projector: &GraphProjector, events: &[PersistedEvent]) {
    for e in events {
        projector.project(e).await.expect("projection failed");
    }
}
