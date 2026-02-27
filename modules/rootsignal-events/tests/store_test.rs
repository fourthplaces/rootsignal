//! Integration tests for EventStore.
//! Requires a Postgres instance. Set DATABASE_TEST_URL or these tests are skipped.

use rootsignal_events::{AppendEvent, EventStore};
use serde_json::json;
use sqlx::PgPool;

/// Get a test database pool, or skip if no test DB is available.
async fn test_pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_TEST_URL").ok()?;
    let pool = PgPool::connect(&url).await.ok()?;

    // Create the events table for testing
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
            schema_v      SMALLINT     NOT NULL DEFAULT 1
        )
        "#,
    )
    .execute(&pool)
    .await
    .ok()?;

    // Clean slate for each test
    sqlx::query("TRUNCATE events RESTART IDENTITY CASCADE")
        .execute(&pool)
        .await
        .ok()?;

    Some(pool)
}

// =========================================================================
// Basic behavior tests
// =========================================================================

#[tokio::test]
async fn append_returns_handle_with_seq() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let handle = store
        .append(AppendEvent::new("test_event", json!({"key": "value"})))
        .await
        .unwrap();

    assert!(handle.seq() > 0);
}

#[tokio::test]
async fn child_event_sets_parent_and_caused_by() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let root = store
        .append(AppendEvent::new("root_event", json!({"step": "root"})))
        .await
        .unwrap();

    let child = root
        .append(AppendEvent::new("child_event", json!({"step": "child"})))
        .await
        .unwrap();

    let grandchild = child
        .append(AppendEvent::new(
            "grandchild_event",
            json!({"step": "grandchild"}),
        ))
        .await
        .unwrap();

    // Verify the causal chain
    let root_event = store.read_event(root.seq()).await.unwrap().unwrap();
    assert!(root_event.parent_seq.is_none());
    assert!(root_event.caused_by_seq.is_none());

    let child_event = store.read_event(child.seq()).await.unwrap().unwrap();
    assert_eq!(child_event.parent_seq, Some(root.seq()));
    assert_eq!(child_event.caused_by_seq, Some(root.seq())); // caused_by = root

    let gc_event = store.read_event(grandchild.seq()).await.unwrap().unwrap();
    assert_eq!(gc_event.parent_seq, Some(child.seq()));
    assert_eq!(gc_event.caused_by_seq, Some(root.seq())); // caused_by still = root
}

#[tokio::test]
async fn read_from_returns_events_in_order() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    store
        .append(AppendEvent::new("event_a", json!({"n": 1})))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("event_b", json!({"n": 2})))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("event_c", json!({"n": 3})))
        .await
        .unwrap();

    let events = store.read_from(1, 100).await.unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event_type, "event_a");
    assert_eq!(events[1].event_type, "event_b");
    assert_eq!(events[2].event_type, "event_c");
    assert!(events[0].seq < events[1].seq);
    assert!(events[1].seq < events[2].seq);
}

#[tokio::test]
async fn read_tree_returns_full_causal_chain() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let root = store
        .append(AppendEvent::new(
            "scrape",
            json!({"url": "https://example.com"}),
        ))
        .await
        .unwrap();

    let extract = root
        .append(AppendEvent::new("extraction", json!({"signals": 2})))
        .await
        .unwrap();

    extract
        .append(AppendEvent::new(
            "signal_discovered",
            json!({"title": "Cleanup"}),
        ))
        .await
        .unwrap();

    extract
        .append(AppendEvent::new(
            "signal_discovered",
            json!({"title": "Food Drive"}),
        ))
        .await
        .unwrap();

    let tree = store.read_tree(root.seq()).await.unwrap();
    assert_eq!(tree.len(), 4); // root + extraction + 2 signals
    assert_eq!(tree[0].event_type, "scrape");
    assert_eq!(tree[1].event_type, "extraction");
}

#[tokio::test]
async fn read_children_returns_direct_children_only() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let root = store
        .append(AppendEvent::new("root", json!({})))
        .await
        .unwrap();

    let child = root
        .append(AppendEvent::new("child", json!({})))
        .await
        .unwrap();

    // Grandchild should NOT appear in root's children
    child
        .append(AppendEvent::new("grandchild", json!({})))
        .await
        .unwrap();

    let children = store.read_children(root.seq()).await.unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].event_type, "child");
}

#[tokio::test]
async fn read_by_type_filters_correctly() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    store
        .append(AppendEvent::new("signal_discovered", json!({})))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("url_scraped", json!({})))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("signal_discovered", json!({})))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("citation_recorded", json!({})))
        .await
        .unwrap();

    let signals = store
        .read_by_type("signal_discovered", 1, 100)
        .await
        .unwrap();
    assert_eq!(signals.len(), 2);
    assert!(signals.iter().all(|e| e.event_type == "signal_discovered"));
}

#[tokio::test]
async fn read_by_run_returns_run_events() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    store
        .append(AppendEvent::new("event_a", json!({})).with_run_id("run-1"))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("event_b", json!({})).with_run_id("run-2"))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("event_c", json!({})).with_run_id("run-1"))
        .await
        .unwrap();

    let run1 = store.read_by_run("run-1").await.unwrap();
    assert_eq!(run1.len(), 2);
    assert!(run1.iter().all(|e| e.run_id.as_deref() == Some("run-1")));
}

#[tokio::test]
async fn latest_seq_returns_max() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    assert_eq!(store.latest_seq().await.unwrap(), 0);

    store
        .append(AppendEvent::new("a", json!({})))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("b", json!({})))
        .await
        .unwrap();
    let last = store
        .append(AppendEvent::new("c", json!({})))
        .await
        .unwrap();

    assert_eq!(store.latest_seq().await.unwrap(), last.seq());
}

#[tokio::test]
async fn child_inherits_run_id_and_actor() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let root = store
        .append(
            AppendEvent::new("root", json!({}))
                .with_run_id("run-abc")
                .with_actor("scout"),
        )
        .await
        .unwrap();

    // Child doesn't set run_id/actor — should inherit from parent handle
    let child = root
        .append(AppendEvent::new("child", json!({})))
        .await
        .unwrap();

    let child_event = store.read_event(child.seq()).await.unwrap().unwrap();
    assert_eq!(child_event.run_id.as_deref(), Some("run-abc"));
    assert_eq!(child_event.actor.as_deref(), Some("scout"));
}

// =========================================================================
// Adversarial tests — try to break the implementation
// =========================================================================

#[tokio::test]
async fn read_from_empty_table_returns_empty() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let events = store.read_from(1, 100).await.unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn read_from_beyond_latest_seq_returns_empty() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    store
        .append(AppendEvent::new("a", json!({})))
        .await
        .unwrap();

    // Ask for events starting well past what exists
    let events = store.read_from(99999, 100).await.unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn read_from_respects_limit() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    for i in 0..10 {
        store
            .append(AppendEvent::new("event", json!({"i": i})))
            .await
            .unwrap();
    }

    let events = store.read_from(1, 3).await.unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].seq, 1);
    assert_eq!(events[2].seq, 3);
}

#[tokio::test]
async fn read_from_gap_free_stops_at_gap() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool.clone());

    // Insert events with seq 1, 2, 3
    store
        .append(AppendEvent::new("a", json!({})))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("b", json!({})))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("c", json!({})))
        .await
        .unwrap();

    // Simulate a gap by manually deleting seq=2
    // (In production, gaps come from rolled-back transactions, but deletion simulates it)
    sqlx::query("DELETE FROM events WHERE seq = 2")
        .execute(&pool)
        .await
        .unwrap();

    // read_from should stop at the gap — return only seq=1
    let events = store.read_from(1, 100).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, 1);
}

#[tokio::test]
async fn read_from_gap_at_start_returns_empty() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool.clone());

    // Insert event with seq=1, then delete it to create gap at start
    store
        .append(AppendEvent::new("a", json!({})))
        .await
        .unwrap();
    store
        .append(AppendEvent::new("b", json!({})))
        .await
        .unwrap();

    sqlx::query("DELETE FROM events WHERE seq = 1")
        .execute(&pool)
        .await
        .unwrap();

    // read_from(1, ...) expects seq=1 first but finds seq=2 — gap at start
    let events = store.read_from(1, 100).await.unwrap();
    assert!(
        events.is_empty(),
        "Gap at start should return empty, got {} events",
        events.len()
    );
}

#[tokio::test]
async fn read_event_nonexistent_returns_none() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let result = store.read_event(99999).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn read_tree_nonexistent_root_returns_empty() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let tree = store.read_tree(99999).await.unwrap();
    assert!(tree.is_empty());
}

#[tokio::test]
async fn read_tree_root_only_returns_just_root() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let root = store
        .append(AppendEvent::new("lonely_root", json!({})))
        .await
        .unwrap();

    let tree = store.read_tree(root.seq()).await.unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].event_type, "lonely_root");
}

#[tokio::test]
async fn read_tree_mid_chain_returns_only_that_event() {
    // read_tree uses caused_by_seq which always points to root.
    // Calling read_tree on a non-root event won't find descendants.
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let root = store
        .append(AppendEvent::new("root", json!({})))
        .await
        .unwrap();
    let child = root
        .append(AppendEvent::new("child", json!({})))
        .await
        .unwrap();
    child
        .append(AppendEvent::new("grandchild", json!({})))
        .await
        .unwrap();

    // read_tree(child.seq) — child's caused_by is root, not itself.
    // So WHERE caused_by_seq = child.seq finds nothing.
    // WHERE seq = child.seq finds the child itself.
    let subtree = store.read_tree(child.seq()).await.unwrap();
    assert_eq!(
        subtree.len(),
        1,
        "Mid-chain read_tree should only return the event itself"
    );
    assert_eq!(subtree[0].event_type, "child");
}

#[tokio::test]
async fn read_children_no_children_returns_empty() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let root = store
        .append(AppendEvent::new("root", json!({})))
        .await
        .unwrap();
    let children = store.read_children(root.seq()).await.unwrap();
    assert!(children.is_empty());
}

#[tokio::test]
async fn empty_payload_is_valid() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let handle = store
        .append(AppendEvent::new("empty", json!({})))
        .await
        .unwrap();
    let event = store.read_event(handle.seq()).await.unwrap().unwrap();
    assert_eq!(event.payload, json!({}));
}

#[tokio::test]
async fn deeply_nested_payload_roundtrips() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let deep_payload = json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": {
                        "value": "deep",
                        "array": [1, 2, {"nested": true}]
                    }
                }
            }
        }
    });

    let handle = store
        .append(AppendEvent::new("deep", deep_payload.clone()))
        .await
        .unwrap();
    let event = store.read_event(handle.seq()).await.unwrap().unwrap();
    assert_eq!(event.payload, deep_payload);
}

#[tokio::test]
async fn large_payload_roundtrips() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    // Simulate a signal_discovered with all properties filled
    let large_payload = json!({
        "type": "signal_discovered",
        "signal_id": "550e8400-e29b-41d4-a716-446655440000",
        "node_type": "gathering",
        "title": "A".repeat(500),
        "summary": "B".repeat(2000),
        "sensitivity": "general",
        "confidence": 0.85,
        "source_url": "https://example.com/really/long/path/to/article",
        "extracted_at": "2026-02-25T12:00:00Z",
        "published_at": "2026-02-25T12:00:00Z",
        "about_location": {"lat": 44.9778, "lng": -93.265, "precision": "neighborhood"},
        "about_location_name": "Minneapolis",
        "implied_queries": (0..20).map(|i| format!("query {i}")).collect::<Vec<_>>(),
        "mentioned_actors": (0..10).map(|i| format!("Actor {i}")).collect::<Vec<_>>(),
        "starts_at": "2026-03-01T10:00:00Z",
        "action_url": "https://example.com/signup",
        "organizer": "Community Council",
        "is_recurring": true,
    });

    let handle = store
        .append(AppendEvent::new("signal_discovered", large_payload.clone()))
        .await
        .unwrap();
    let event = store.read_event(handle.seq()).await.unwrap().unwrap();
    assert_eq!(event.payload, large_payload);
}

#[tokio::test]
async fn schema_v_persists_correctly() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let handle = store
        .append(AppendEvent::new("v2_event", json!({"new_field": "added"})).with_schema_v(2))
        .await
        .unwrap();

    let event = store.read_event(handle.seq()).await.unwrap().unwrap();
    assert_eq!(event.schema_v, 2);
}

#[tokio::test]
async fn multiple_roots_create_independent_trees() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let root_a = store
        .append(AppendEvent::new("root_a", json!({})))
        .await
        .unwrap();
    let root_b = store
        .append(AppendEvent::new("root_b", json!({})))
        .await
        .unwrap();

    root_a
        .append(AppendEvent::new("child_a", json!({})))
        .await
        .unwrap();
    root_b
        .append(AppendEvent::new("child_b", json!({})))
        .await
        .unwrap();
    root_a
        .append(AppendEvent::new("child_a2", json!({})))
        .await
        .unwrap();

    let tree_a = store.read_tree(root_a.seq()).await.unwrap();
    let tree_b = store.read_tree(root_b.seq()).await.unwrap();

    assert_eq!(tree_a.len(), 3); // root_a + child_a + child_a2
    assert_eq!(tree_b.len(), 2); // root_b + child_b

    // No cross-contamination
    assert!(tree_a
        .iter()
        .all(|e| e.event_type.contains("_a") || e.event_type == "root_a"));
    assert!(tree_b
        .iter()
        .all(|e| e.event_type.contains("_b") || e.event_type == "root_b"));
}

#[tokio::test]
async fn read_by_type_nonexistent_type_returns_empty() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    store
        .append(AppendEvent::new("signal_discovered", json!({})))
        .await
        .unwrap();

    let results = store.read_by_type("does_not_exist", 1, 100).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn read_by_run_nonexistent_run_returns_empty() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    store
        .append(AppendEvent::new("a", json!({})).with_run_id("run-1"))
        .await
        .unwrap();

    let results = store.read_by_run("does-not-exist").await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn child_can_override_inherited_actor() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let root = store
        .append(AppendEvent::new("root", json!({})).with_actor("scout"))
        .await
        .unwrap();

    // Child explicitly sets a different actor
    let child = root
        .append(AppendEvent::new("child", json!({})).with_actor("supervisor"))
        .await
        .unwrap();

    let child_event = store.read_event(child.seq()).await.unwrap().unwrap();
    assert_eq!(child_event.actor.as_deref(), Some("supervisor"));
}

#[tokio::test]
async fn sequential_seqs_are_monotonically_increasing() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let mut seqs = Vec::new();
    for _ in 0..20 {
        let handle = store
            .append(AppendEvent::new("event", json!({})))
            .await
            .unwrap();
        seqs.push(handle.seq());
    }

    for window in seqs.windows(2) {
        assert!(
            window[1] > window[0],
            "Seqs must be monotonically increasing: {} should be > {}",
            window[1],
            window[0]
        );
    }
}

#[tokio::test]
async fn read_from_pagination_works() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    for i in 0..10 {
        store
            .append(AppendEvent::new("event", json!({"i": i})))
            .await
            .unwrap();
    }

    // Page 1: events 1-3
    let page1 = store.read_from(1, 3).await.unwrap();
    assert_eq!(page1.len(), 3);

    // Page 2: events 4-6 (start from last seq + 1)
    let page2 = store
        .read_from(page1.last().unwrap().seq + 1, 3)
        .await
        .unwrap();
    assert_eq!(page2.len(), 3);
    assert_eq!(page2[0].seq, 4);

    // Page 3: events 7-9
    let page3 = store
        .read_from(page2.last().unwrap().seq + 1, 3)
        .await
        .unwrap();
    assert_eq!(page3.len(), 3);
    assert_eq!(page3[0].seq, 7);

    // Page 4: event 10 (partial page)
    let page4 = store
        .read_from(page3.last().unwrap().seq + 1, 3)
        .await
        .unwrap();
    assert_eq!(page4.len(), 1);
    assert_eq!(page4[0].seq, 10);

    // Page 5: empty
    let page5 = store
        .read_from(page4.last().unwrap().seq + 1, 3)
        .await
        .unwrap();
    assert!(page5.is_empty());
}

#[tokio::test]
async fn unicode_and_special_chars_in_payload() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);

    let payload = json!({
        "title": "Limpieza del vecindario \u{1F30E}",
        "summary": "日本語テスト — \"quotes\" & <brackets> 'apostrophes'",
        "emoji": "\u{1F4A5}\u{1F525}\u{2764}\u{FE0F}",
        "null_field": null,
        "empty_string": "",
        "zero": 0,
        "false_bool": false,
    });

    let handle = store
        .append(AppendEvent::new("unicode_test", payload.clone()))
        .await
        .unwrap();
    let event = store.read_event(handle.seq()).await.unwrap().unwrap();
    assert_eq!(event.payload, payload);
}
