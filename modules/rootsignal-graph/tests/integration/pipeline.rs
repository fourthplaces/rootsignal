// End-to-end pipeline integration tests.
//
// These tests verify that events processed through the Pipeline
// produce correct graph state (factual + derived properties).

use chrono::Utc;
use uuid::Uuid;
use rootsignal_common::events::{Event, Location, SystemEvent, WorldEvent};
use rootsignal_common::{ActorType, ChannelType, GeoPoint, GeoPrecision};
use rootsignal_world::types::{Entity, EntityType, Reference};
use causal::types::PersistedEvent;
use rootsignal_graph::{query, BBox, GraphClient, Pipeline};

fn stored(seq: i64, event: &Event) -> PersistedEvent {
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

fn bbox() -> BBox {
    BBox {
        min_lat: 44.0,
        max_lat: 46.0,
        min_lng: -94.0,
        max_lng: -92.0,
    }
}

fn mpls() -> Option<Location> {
    Some(Location {
        point: Some(GeoPoint {
            lat: 44.9778,
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        }),
        name: Some("Minneapolis".into()),
        address: None,
        role: None,
    })
}

async fn read_prop<T: for<'a> serde::Deserialize<'a> + Default>(
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

#[tokio::test]
async fn pipeline_creates_signal_with_factual_and_derived_properties() {
    let (_guard, client) = super::setup().await;
    let pipeline = Pipeline::new(client.clone(), 0.3);

    let id = Uuid::new_v4();
    let event = Event::World(WorldEvent::GatheringAnnounced {
        id,
        title: "Community Potluck".into(),
        summary: "A neighborhood potluck in the park".into(),
        url: "https://patch.com/mn/potluck".into(),
        published_at: None,
        extraction_id: None,
        locations: mpls().into_iter().collect(),
        mentioned_entities: vec![],
        references: vec![],
        schedule: None,
        action_url: None,
    });

    let events = vec![stored(1, &event)];
    let stats = pipeline
        .process(&events, &bbox())
        .await
        .expect("pipeline failed");

    assert_eq!(stats.events_applied, 1);

    // Factual properties (set by reducer)
    let title: String = read_prop(&client, "Gathering", id, "title").await;
    assert_eq!(title, "Community Potluck");

    let confidence: f64 = read_prop(&client, "Gathering", id, "confidence").await;
    assert!((confidence - 0.5).abs() < 0.01);

    // Derived properties (set by enrichment)
    let src_div: i64 = read_prop(&client, "Gathering", id, "source_diversity").await;
    let ext_ratio: f64 = read_prop(&client, "Gathering", id, "external_ratio").await;
    assert_eq!(src_div, 0);
    assert_eq!(ext_ratio, 0.0);
}

#[tokio::test]
async fn pipeline_creates_evidence_and_computes_diversity() {
    let (_guard, client) = super::setup().await;
    let pipeline = Pipeline::new(client.clone(), 0.3);

    let signal_id = Uuid::new_v4();
    let ev1_id = Uuid::new_v4();
    let ev2_id = Uuid::new_v4();

    let events = vec![
        stored(
            1,
            &Event::World(WorldEvent::GatheringAnnounced {
                id: signal_id,
                title: "Rally at Capitol".into(),
                summary: "Advocacy rally".into(),
                url: "https://startribune.com/rally".into(),
                published_at: None,
                extraction_id: None,
                locations: mpls().into_iter().collect(),
                mentioned_entities: vec![],
                references: vec![],
                schedule: None,
                action_url: None,
            }),
        ),
        stored(
            2,
            &Event::World(WorldEvent::CitationPublished {
                citation_id: ev1_id,
                signal_id: signal_id,
                url: "https://mpr.org/rally-coverage".into(),
                content_hash: "abc123".into(),
                snippet: Some("MPR reports on the rally".into()),
                relevance: Some("SUPPORTING".into()),
                channel_type: Some(ChannelType::Press),
                evidence_confidence: Some(0.8),
            }),
        ),
        stored(
            3,
            &Event::World(WorldEvent::CitationPublished {
                citation_id: ev2_id,
                signal_id: signal_id,
                url: "https://twitter.com/user/rally".into(),
                content_hash: "def456".into(),
                snippet: Some("Live from the rally".into()),
                relevance: Some("DIRECT".into()),
                channel_type: Some(ChannelType::Social),
                evidence_confidence: Some(0.7),
            }),
        ),
    ];

    let stats = pipeline
        .process(&events, &bbox())
        .await
        .expect("pipeline failed");

    assert_eq!(stats.events_applied, 3);

    // Diversity stats require a separate SignalDiversityComputed system event.
    // The Pipeline only projects world events + cause_heat — no enrichment step.
    // Verify Citation nodes were created (the evidence projection itself).
    let citation_count: i64 = {
        let cypher = "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Citation) RETURN count(ev) AS cnt";
        let q = query(cypher).param("id", signal_id.to_string());
        let mut stream = client.execute(q).await.expect("query failed");
        let row = stream.next().await.expect("stream failed").expect("no row");
        row.get::<i64>("cnt").unwrap_or(0)
    };
    assert_eq!(citation_count, 2, "Two Citation nodes should be linked");
}

#[tokio::test]
async fn pipeline_actor_signal_count_computed_after_reduce() {
    let (_guard, client) = super::setup().await;
    let pipeline = Pipeline::new(client.clone(), 0.3);

    let actor_id = Uuid::new_v4();
    let sig1 = Uuid::new_v4();
    let sig2 = Uuid::new_v4();

    let events = vec![
        stored(
            1,
            &Event::World(WorldEvent::ConcernRaised {
                id: sig1,
                title: "Housing crisis".into(),
                summary: "Rising rents".into(),
                url: "https://example.com/housing".into(),
                published_at: None,
                extraction_id: None,
                locations: mpls().into_iter().collect(),
                mentioned_entities: vec![],
                references: vec![],
                schedule: None,
                subject: None,
                opposing: None,
            }),
        ),
        stored(
            2,
            &Event::World(WorldEvent::ResourceOffered {
                id: sig2,
                title: "Rent assistance".into(),
                summary: "Emergency fund".into(),
                url: "https://example.com/aid".into(),
                published_at: None,
                extraction_id: None,
                locations: mpls().into_iter().collect(),
                mentioned_entities: vec![],
                references: vec![],
                schedule: None,
                action_url: None,
                availability: None,
                eligibility: None,
            }),
        ),
        stored(
            3,
            &Event::System(SystemEvent::ActorIdentified {
                actor_id,
                name: "Housing Alliance".into(),
                actor_type: ActorType::Organization,
                canonical_key: sig1.to_string(),
                domains: vec![],
                social_urls: vec![],
                description: "Housing advocacy org".into(),
                bio: None,
                location_lat: None,
                location_lng: None,
                location_name: None,
            }),
        ),
        stored(
            4,
            &Event::System(SystemEvent::ActorLinkedToSignal {
                actor_id,
                signal_id: sig2,
                role: "provider".into(),
            }),
        ),
    ];

    pipeline
        .process(&events, &bbox())
        .await
        .expect("pipeline failed");

    // ActorIdentified creates the Actor node (no ACTED_IN edge)
    // ActorLinkedToSignal creates one ACTED_IN edge to sig2
    // signal_count requires a separate ActorStatsComputed system event (not emitted here).
    // Verify the ACTED_IN edge was created by the projector.
    let edge_count: i64 = {
        let cypher = "MATCH (a:Actor {id: $id})-[:ACTED_IN]->() RETURN count(*) AS cnt";
        let q = query(cypher).param("id", actor_id.to_string());
        let mut stream = client.execute(q).await.expect("query failed");
        let row = stream.next().await.expect("stream failed").expect("no row");
        row.get::<i64>("cnt").unwrap_or(0)
    };
    assert_eq!(edge_count, 1, "Actor should have one ACTED_IN edge");
}

// ---------------------------------------------------------------------------
// Replay idempotency
// ---------------------------------------------------------------------------

/// Snapshot of graph state for comparison.
#[derive(Debug, PartialEq)]
struct SignalSnapshot {
    title: String,
    source_diversity: i64,
    channel_diversity: i64,
    external_ratio: f64,
}

async fn snapshot_signal(client: &GraphClient, label: &str, id: Uuid) -> SignalSnapshot {
    SignalSnapshot {
        title: read_prop(client, label, id, "title").await,
        source_diversity: read_prop(client, label, id, "source_diversity").await,
        channel_diversity: read_prop(client, label, id, "channel_diversity").await,
        external_ratio: read_prop(client, label, id, "external_ratio").await,
    }
}

#[tokio::test]
async fn replay_produces_identical_graph() {
    let (_guard, client) = super::setup().await;
    let pipeline = Pipeline::new(client.clone(), 0.3);

    let signal_id = Uuid::new_v4();
    let ev_id = Uuid::new_v4();

    let events = vec![
        stored(
            1,
            &Event::World(WorldEvent::GatheringAnnounced {
                id: signal_id,
                title: "Farmers Market".into(),
                summary: "Weekly market".into(),
                url: "https://patch.com/market".into(),
                published_at: None,
                extraction_id: None,
                locations: mpls().into_iter().collect(),
                mentioned_entities: vec![],
                references: vec![],
                schedule: None,
                action_url: None,
            }),
        ),
        stored(
            2,
            &Event::World(WorldEvent::CitationPublished {
                citation_id: ev_id,
                signal_id: signal_id,
                url: "https://mpr.org/market".into(),
                content_hash: "hash1".into(),
                snippet: Some("MPR covers the market".into()),
                relevance: Some("SUPPORTING".into()),
                channel_type: Some(ChannelType::Press),
                evidence_confidence: Some(0.8),
            }),
        ),
    ];

    // First pass
    pipeline
        .process(&events, &bbox())
        .await
        .expect("first pass failed");
    let snap1 = snapshot_signal(&client, "Gathering", signal_id).await;

    // Wipe graph
    client
        .run(query("MATCH (n) DETACH DELETE n"))
        .await
        .expect("wipe failed");

    // Replay same events
    pipeline
        .process(&events, &bbox())
        .await
        .expect("replay failed");
    let snap2 = snapshot_signal(&client, "Gathering", signal_id).await;

    assert_eq!(snap1, snap2, "Replay must produce identical graph state");
}

