//! Integration tests for PostgresSnapshotStore + EventStore partial replay.
//! Requires a Postgres instance. Set DATABASE_TEST_URL or these tests are skipped.

use chrono::Utc;
use rootsignal_events::{AppendEvent, EventStore, PostgresSnapshotStore};
use seesaw_core::{Snapshot, SnapshotStore};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_TEST_URL").ok()?;
    let pool = PgPool::connect(&url).await.ok()?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS events (
            seq           BIGSERIAL    PRIMARY KEY,
            ts            TIMESTAMPTZ  NOT NULL DEFAULT now(),
            event_type    TEXT         NOT NULL,
            parent_seq    BIGINT       REFERENCES events(seq),
            caused_by_seq BIGINT       REFERENCES events(seq),
            run_id        TEXT,
            actor         TEXT,
            payload       JSONB        NOT NULL,
            schema_v      SMALLINT     NOT NULL DEFAULT 1,
            id            UUID,
            parent_id     UUID,
            correlation_id UUID,
            aggregate_type TEXT,
            aggregate_id   UUID
        )
        "#,
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS aggregate_snapshots (
            aggregate_type TEXT        NOT NULL,
            aggregate_id   UUID        NOT NULL,
            version        BIGINT      NOT NULL,
            state          JSONB       NOT NULL,
            created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY (aggregate_type, aggregate_id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query("TRUNCATE events RESTART IDENTITY CASCADE")
        .execute(&pool)
        .await
        .ok()?;
    sqlx::query("TRUNCATE aggregate_snapshots")
        .execute(&pool)
        .await
        .ok()?;

    Some(pool)
}

fn test_event(event_type: &str, agg_id: Uuid) -> AppendEvent {
    AppendEvent {
        event_type: event_type.to_string(),
        payload: json!({"type": event_type}),
        run_id: Some("test-run".to_string()),
        actor: None,
        schema_v: 1,
        id: Some(Uuid::new_v4()),
        parent_id: None,
        correlation_id: None,
        aggregate_type: Some("TestAggregate".to_string()),
        aggregate_id: Some(agg_id),
    }
}

// =========================================================================
// Snapshot round-trip
// =========================================================================

#[tokio::test]
async fn snapshot_round_trips_through_postgres() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = PostgresSnapshotStore::new(pool);
    let agg_id = Uuid::new_v4();

    let snapshot = Snapshot {
        aggregate_type: "TestAggregate".to_string(),
        aggregate_id: agg_id,
        version: 42,
        state: json!({"count": 10, "name": "test"}),
        created_at: Utc::now(),
    };

    store.save_snapshot(snapshot.clone()).await.unwrap();
    let loaded = store
        .load_snapshot("TestAggregate", agg_id)
        .await
        .unwrap();

    let loaded = loaded.expect("snapshot should exist");
    assert_eq!(loaded.aggregate_type, "TestAggregate");
    assert_eq!(loaded.aggregate_id, agg_id);
    assert_eq!(loaded.version, 42);
    assert_eq!(loaded.state, json!({"count": 10, "name": "test"}));
}

#[tokio::test]
async fn missing_snapshot_returns_none() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = PostgresSnapshotStore::new(pool);

    let loaded = store
        .load_snapshot("TestAggregate", Uuid::new_v4())
        .await
        .unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn save_snapshot_overwrites_older_version() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = PostgresSnapshotStore::new(pool);
    let agg_id = Uuid::new_v4();

    store
        .save_snapshot(Snapshot {
            aggregate_type: "TestAggregate".to_string(),
            aggregate_id: agg_id,
            version: 10,
            state: json!({"count": 5}),
            created_at: Utc::now(),
        })
        .await
        .unwrap();

    store
        .save_snapshot(Snapshot {
            aggregate_type: "TestAggregate".to_string(),
            aggregate_id: agg_id,
            version: 20,
            state: json!({"count": 15}),
            created_at: Utc::now(),
        })
        .await
        .unwrap();

    let loaded = store
        .load_snapshot("TestAggregate", agg_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.version, 20);
    assert_eq!(loaded.state, json!({"count": 15}));
}

// =========================================================================
// Partial replay from snapshot point
// =========================================================================

#[tokio::test]
async fn partial_replay_from_snapshot_returns_only_remaining_events() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let event_store = EventStore::new(pool.clone());
    let snapshot_store = PostgresSnapshotStore::new(pool);
    let agg_id = Uuid::new_v4();

    // Append 5 events for the same aggregate
    let mut seqs = Vec::new();
    for i in 1..=5 {
        let handle = event_store
            .append(test_event(&format!("event_{i}"), agg_id))
            .await
            .unwrap();
        seqs.push(handle.seq());
    }

    // Snapshot at event 3 — means events 1-3 are "baked in"
    let snapshot_seq = seqs[2];
    snapshot_store
        .save_snapshot(Snapshot {
            aggregate_type: "TestAggregate".to_string(),
            aggregate_id: agg_id,
            version: snapshot_seq as u64,
            state: json!({"count": 3}),
            created_at: Utc::now(),
        })
        .await
        .unwrap();

    // Full replay: all 5 events
    let full = event_store
        .load_aggregate_stream("TestAggregate", agg_id)
        .await
        .unwrap();
    assert_eq!(full.len(), 5);

    // Partial replay from snapshot: only events 4 and 5
    let partial = event_store
        .load_aggregate_stream_from("TestAggregate", agg_id, snapshot_seq)
        .await
        .unwrap();
    assert_eq!(partial.len(), 2);
    assert_eq!(partial[0].event_type, "event_4");
    assert_eq!(partial[1].event_type, "event_5");
}

#[tokio::test]
async fn partial_replay_with_no_remaining_events_returns_empty() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let event_store = EventStore::new(pool.clone());
    let agg_id = Uuid::new_v4();

    // Append 3 events
    let mut last_seq = 0;
    for i in 1..=3 {
        let handle = event_store
            .append(test_event(&format!("event_{i}"), agg_id))
            .await
            .unwrap();
        last_seq = handle.seq();
    }

    // Snapshot at the last event — nothing remaining
    let partial = event_store
        .load_aggregate_stream_from("TestAggregate", agg_id, last_seq)
        .await
        .unwrap();
    assert!(partial.is_empty());
}

#[tokio::test]
async fn partial_replay_excludes_other_aggregates() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let event_store = EventStore::new(pool.clone());
    let agg_a = Uuid::new_v4();
    let agg_b = Uuid::new_v4();

    // Interleave events from two aggregates
    let h1 = event_store
        .append(test_event("a_event_1", agg_a))
        .await
        .unwrap();
    let _h2 = event_store
        .append(test_event("b_event_1", agg_b))
        .await
        .unwrap();
    let _h3 = event_store
        .append(test_event("a_event_2", agg_a))
        .await
        .unwrap();
    let _h4 = event_store
        .append(test_event("b_event_2", agg_b))
        .await
        .unwrap();

    // Snapshot agg_a at event 1 — partial replay should only return a_event_2
    let partial = event_store
        .load_aggregate_stream_from("TestAggregate", agg_a, h1.seq())
        .await
        .unwrap();
    assert_eq!(partial.len(), 1);
    assert_eq!(partial[0].event_type, "a_event_2");
}
