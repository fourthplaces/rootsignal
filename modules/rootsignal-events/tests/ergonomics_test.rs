//! Ergonomics and usage pattern tests.
//! These don't need Postgres — they test the API surface and developer experience.

use rootsignal_events::AppendEvent;
use serde_json::json;

// =========================================================================
// AppendEvent builder ergonomics
// =========================================================================

#[test]
fn append_event_minimal_construction() {
    let event = AppendEvent::new("signal_discovered", json!({"title": "Test"}));
    assert_eq!(event.event_type, "signal_discovered");
    assert!(event.run_id.is_none());
    assert!(event.actor.is_none());
    assert_eq!(event.schema_v, 1);
}

#[test]
fn append_event_full_builder_chain() {
    let event = AppendEvent::new("signal_discovered", json!({"title": "Test"}))
        .with_run_id("run-abc-123")
        .with_actor("scout")
        .with_schema_v(2);

    assert_eq!(event.event_type, "signal_discovered");
    assert_eq!(event.run_id.as_deref(), Some("run-abc-123"));
    assert_eq!(event.actor.as_deref(), Some("scout"));
    assert_eq!(event.schema_v, 2);
}

#[test]
fn append_event_builder_order_doesnt_matter() {
    let a = AppendEvent::new("test", json!({}))
        .with_run_id("run")
        .with_actor("scout");

    let b = AppendEvent::new("test", json!({}))
        .with_actor("scout")
        .with_run_id("run");

    assert_eq!(a.run_id, b.run_id);
    assert_eq!(a.actor, b.actor);
}

#[test]
fn stored_event_is_serializable() {
    // StoredEvent can be serialized for debugging, logging, admin UI
    let stored = rootsignal_events::StoredEvent {
        seq: 42,
        ts: chrono::Utc::now(),
        event_type: "signal_discovered".to_string(),
        parent_seq: Some(41),
        caused_by_seq: Some(40),
        run_id: Some("run-123".to_string()),
        actor: Some("scout".to_string()),
        payload: json!({"title": "Test Signal"}),
        schema_v: 1,
        id: None,
        parent_id: None,
    };

    let json = serde_json::to_string(&stored).unwrap();
    assert!(json.contains("signal_discovered"));
    assert!(json.contains("42"));

    // And deserializable
    let roundtripped: rootsignal_events::StoredEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtripped.seq, 42);
    assert_eq!(roundtripped.event_type, "signal_discovered");
}
