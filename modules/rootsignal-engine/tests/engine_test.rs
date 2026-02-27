//! Integration tests for Engine dispatch loop.
//! Requires a Postgres instance. Set DATABASE_TEST_URL or these tests are skipped.

use anyhow::Result;
use async_trait::async_trait;
use rootsignal_engine::{Engine, EventLike, Reducer, Router};
use rootsignal_events::{EventStore, StoredEvent};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Test event type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum TestEvent {
    Start { label: String },
    Middle { label: String },
    End { label: String },
}

impl EventLike for TestEvent {
    fn event_type_str(&self) -> String {
        match self {
            TestEvent::Start { .. } => "test:start".into(),
            TestEvent::Middle { .. } => "test:middle".into(),
            TestEvent::End { .. } => "test:end".into(),
        }
    }

    fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("TestEvent serialization should never fail")
    }
}

// ---------------------------------------------------------------------------
// Test state
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct TestState {
    events_seen: Vec<String>,
    start_count: u32,
    middle_count: u32,
    end_count: u32,
}

// ---------------------------------------------------------------------------
// Test reducer
// ---------------------------------------------------------------------------

struct TestReducer;

impl Reducer<TestEvent, TestState> for TestReducer {
    fn reduce(&self, state: &mut TestState, event: &TestEvent) {
        match event {
            TestEvent::Start { label } => {
                state.events_seen.push(label.clone());
                state.start_count += 1;
            }
            TestEvent::Middle { label } => {
                state.events_seen.push(label.clone());
                state.middle_count += 1;
            }
            TestEvent::End { label } => {
                state.events_seen.push(label.clone());
                state.end_count += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Test router: Start → Middle → End (chain of 3)
// ---------------------------------------------------------------------------

struct ChainingRouter;

#[async_trait]
impl Router<TestEvent, TestState, ()> for ChainingRouter {
    async fn route(
        &self,
        event: &TestEvent,
        _stored: &StoredEvent,
        _state: &mut TestState,
        _deps: &(),
    ) -> Result<Vec<TestEvent>> {
        match event {
            TestEvent::Start { label } => Ok(vec![TestEvent::Middle {
                label: format!("{label}→middle"),
            }]),
            TestEvent::Middle { label } => Ok(vec![TestEvent::End {
                label: format!("{label}→end"),
            }]),
            TestEvent::End { .. } => Ok(vec![]),
        }
    }
}

// ---------------------------------------------------------------------------
// No-op router (no children emitted)
// ---------------------------------------------------------------------------

struct NoOpRouter;

#[async_trait]
impl Router<TestEvent, TestState, ()> for NoOpRouter {
    async fn route(
        &self,
        _event: &TestEvent,
        _stored: &StoredEvent,
        _state: &mut TestState,
        _deps: &(),
    ) -> Result<Vec<TestEvent>> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// Fan-out router: Start emits 3 children
// ---------------------------------------------------------------------------

struct FanOutRouter;

#[async_trait]
impl Router<TestEvent, TestState, ()> for FanOutRouter {
    async fn route(
        &self,
        event: &TestEvent,
        _stored: &StoredEvent,
        _state: &mut TestState,
        _deps: &(),
    ) -> Result<Vec<TestEvent>> {
        match event {
            TestEvent::Start { .. } => Ok(vec![
                TestEvent::End {
                    label: "child-1".into(),
                },
                TestEvent::End {
                    label: "child-2".into(),
                },
                TestEvent::End {
                    label: "child-3".into(),
                },
            ]),
            _ => Ok(vec![]),
        }
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

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
            schema_v      SMALLINT     NOT NULL DEFAULT 1
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

    Some(pool)
}

// =========================================================================
// Tests
// =========================================================================

#[tokio::test]
async fn single_event_persists_and_reduces_state() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);
    let engine = Engine::new(TestReducer, NoOpRouter, store.clone(), "test-run".into());

    let mut state = TestState::default();
    engine
        .dispatch(
            TestEvent::Start {
                label: "hello".into(),
            },
            &mut state,
            &(),
        )
        .await
        .unwrap();

    assert_eq!(state.start_count, 1);
    assert_eq!(state.events_seen, vec!["hello"]);

    // Verify persisted
    let events = store.read_by_run("test-run").await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "test:start");
}

#[tokio::test]
async fn chained_events_form_causal_tree() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);
    let engine = Engine::new(
        TestReducer,
        ChainingRouter,
        store.clone(),
        "chain-run".into(),
    );

    let mut state = TestState::default();
    engine
        .dispatch(
            TestEvent::Start {
                label: "root".into(),
            },
            &mut state,
            &(),
        )
        .await
        .unwrap();

    // Reducer saw all 3 events in order
    assert_eq!(state.start_count, 1);
    assert_eq!(state.middle_count, 1);
    assert_eq!(state.end_count, 1);
    assert_eq!(
        state.events_seen,
        vec!["root", "root→middle", "root→middle→end"]
    );

    // All 3 persisted with causal chain
    let events = store.read_by_run("chain-run").await.unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event_type, "test:start");
    assert_eq!(events[1].event_type, "test:middle");
    assert_eq!(events[2].event_type, "test:end");

    // Root has no parent
    assert!(events[0].parent_seq.is_none());
    assert!(events[0].caused_by_seq.is_none());

    // Middle is child of root
    assert_eq!(events[1].parent_seq, Some(events[0].seq));
    assert_eq!(events[1].caused_by_seq, Some(events[0].seq));

    // End is child of middle, caused_by still root
    assert_eq!(events[2].parent_seq, Some(events[1].seq));
    assert_eq!(events[2].caused_by_seq, Some(events[0].seq));

    // read_tree from root returns complete chain
    let tree = store.read_tree(events[0].seq).await.unwrap();
    assert_eq!(tree.len(), 3);
}

#[tokio::test]
async fn fan_out_creates_sibling_children() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);
    let engine = Engine::new(TestReducer, FanOutRouter, store.clone(), "fan-run".into());

    let mut state = TestState::default();
    engine
        .dispatch(
            TestEvent::Start {
                label: "root".into(),
            },
            &mut state,
            &(),
        )
        .await
        .unwrap();

    assert_eq!(state.start_count, 1);
    assert_eq!(state.end_count, 3);
    assert_eq!(
        state.events_seen,
        vec!["root", "child-1", "child-2", "child-3"]
    );

    // All children share the same parent
    let events = store.read_by_run("fan-run").await.unwrap();
    assert_eq!(events.len(), 4);

    let root_seq = events[0].seq;
    let children = store.read_children(root_seq).await.unwrap();
    assert_eq!(children.len(), 3);
    assert!(children.iter().all(|c| c.parent_seq == Some(root_seq)));
    assert!(children.iter().all(|c| c.caused_by_seq == Some(root_seq)));
}

#[tokio::test]
async fn dispatch_processes_breadth_first() {
    // With a fan-out + chaining combo, verify BFS order.
    // Start → [Middle-A, Middle-B] → [End-A, End-B]
    // BFS: Start, Middle-A, Middle-B, End-A, End-B

    struct BfsRouter;

    #[async_trait]
    impl Router<TestEvent, TestState, ()> for BfsRouter {
        async fn route(
            &self,
            event: &TestEvent,
            _stored: &StoredEvent,
            _state: &mut TestState,
            _deps: &(),
        ) -> Result<Vec<TestEvent>> {
            match event {
                TestEvent::Start { .. } => Ok(vec![
                    TestEvent::Middle { label: "A".into() },
                    TestEvent::Middle { label: "B".into() },
                ]),
                TestEvent::Middle { label } => Ok(vec![TestEvent::End {
                    label: format!("end-{label}"),
                }]),
                TestEvent::End { .. } => Ok(vec![]),
            }
        }
    }

    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);
    let engine = Engine::new(TestReducer, BfsRouter, store.clone(), "bfs-run".into());

    let mut state = TestState::default();
    engine
        .dispatch(
            TestEvent::Start {
                label: "root".into(),
            },
            &mut state,
            &(),
        )
        .await
        .unwrap();

    // BFS order: root, A, B, end-A, end-B
    assert_eq!(state.events_seen, vec!["root", "A", "B", "end-A", "end-B"]);
}

#[tokio::test]
async fn run_id_set_on_all_persisted_events() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let store = EventStore::new(pool);
    let engine = Engine::new(
        TestReducer,
        ChainingRouter,
        store.clone(),
        "my-run-123".into(),
    );

    let mut state = TestState::default();
    engine
        .dispatch(TestEvent::Start { label: "go".into() }, &mut state, &())
        .await
        .unwrap();

    let events = store.read_by_run("my-run-123").await.unwrap();
    assert_eq!(events.len(), 3);
    assert!(events
        .iter()
        .all(|e| e.run_id.as_deref() == Some("my-run-123")));
}
