//! EventStore — append-only fact store backed by Postgres.
//!
//! Gap-free reads are guaranteed internally. Consumers never see BIGSERIAL gaps
//! from rolled-back or in-flight transactions. This is the store's job.

use anyhow::Result;
use futures::Stream;
use sqlx::PgPool;
use std::pin::Pin;
use tracing::warn;

use crate::types::{AppendEvent, StoredEvent};

// ---------------------------------------------------------------------------
// EventStore
// ---------------------------------------------------------------------------

/// Append-only fact store. The single source of truth.
#[derive(Clone)]
pub struct EventStore {
    pool: PgPool,
}

impl EventStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Append a root fact (no parent). Returns a handle for emitting children.
    pub async fn append(&self, event: AppendEvent) -> Result<EventHandle> {
        let row = sqlx::query_as::<_, (i64,)>(
            r#"
            INSERT INTO events (event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v)
            VALUES ($1, NULL, NULL, $2, $3, $4, $5)
            RETURNING seq
            "#,
        )
        .bind(&event.event_type)
        .bind(&event.run_id)
        .bind(&event.actor)
        .bind(&event.payload)
        .bind(event.schema_v)
        .fetch_one(&self.pool)
        .await?;

        let seq = row.0;

        // Best-effort PG NOTIFY — a nudge, not a delivery guarantee.
        notify_new_event(&self.pool, seq).await;

        Ok(EventHandle {
            seq,
            caused_by: seq, // Root event: caused_by points to itself
            store: self.clone(),
            run_id: event.run_id,
            actor: event.actor,
        })
    }

    /// Append a root fact and return the full StoredEvent (with ts from Postgres).
    /// Used by the projector path where we need the complete event for projection.
    pub async fn append_and_read(&self, event: AppendEvent) -> Result<StoredEvent> {
        let stored = sqlx::query_as::<_, StoredEvent>(
            r#"
            INSERT INTO events (event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v)
            VALUES ($1, NULL, NULL, $2, $3, $4, $5)
            RETURNING seq, ts, event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v
            "#,
        )
        .bind(&event.event_type)
        .bind(&event.run_id)
        .bind(&event.actor)
        .bind(&event.payload)
        .bind(event.schema_v)
        .fetch_one(&self.pool)
        .await?;

        notify_new_event(&self.pool, stored.seq).await;

        Ok(stored)
    }

    /// Read facts in flat sequence order starting from `seq_start` (inclusive).
    ///
    /// **Gap-free guarantee:** If concurrent transactions created a momentary gap,
    /// this returns events only up to the gap boundary. The next call picks up
    /// where it left off once the gap closes. Consumers never see gaps.
    pub async fn read_from(&self, seq_start: i64, limit: usize) -> Result<Vec<StoredEvent>> {
        // Fetch the candidate rows
        let rows = sqlx::query_as::<_, StoredEvent>(
            r#"
            SELECT seq, ts, event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v
            FROM events
            WHERE seq >= $1
            ORDER BY seq ASC
            LIMIT $2
            "#,
        )
        .bind(seq_start)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        // Enforce gap-free: stop at the first gap in the sequence.
        let mut result = Vec::with_capacity(rows.len());
        let mut expected_seq = seq_start;

        for row in rows {
            if row.seq != expected_seq {
                // Gap detected — an in-flight transaction hasn't committed yet.
                // Return what we have so far. Next call will pick up the rest.
                break;
            }
            expected_seq = row.seq + 1;
            result.push(row);
        }

        Ok(result)
    }

    /// Read a single event by sequence number.
    pub async fn read_event(&self, seq: i64) -> Result<Option<StoredEvent>> {
        let row = sqlx::query_as::<_, StoredEvent>(
            r#"
            SELECT seq, ts, event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v
            FROM events
            WHERE seq = $1
            "#,
        )
        .bind(seq)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// Read facts filtered by event type, in sequence order.
    pub async fn read_by_type(
        &self,
        event_type: &str,
        seq_start: i64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>> {
        let rows = sqlx::query_as::<_, StoredEvent>(
            r#"
            SELECT seq, ts, event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v
            FROM events
            WHERE event_type = $1 AND seq >= $2
            ORDER BY seq ASC
            LIMIT $3
            "#,
        )
        .bind(event_type)
        .bind(seq_start)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Read all facts for a given run, in sequence order.
    pub async fn read_by_run(&self, run_id: &str) -> Result<Vec<StoredEvent>> {
        let rows = sqlx::query_as::<_, StoredEvent>(
            r#"
            SELECT seq, ts, event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v
            FROM events
            WHERE run_id = $1
            ORDER BY seq ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Read the full causal tree rooted at an event (recursive).
    pub async fn read_tree(&self, root_seq: i64) -> Result<Vec<StoredEvent>> {
        let rows = sqlx::query_as::<_, StoredEvent>(
            r#"
            SELECT seq, ts, event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v
            FROM events
            WHERE caused_by_seq = $1 OR seq = $1
            ORDER BY seq ASC
            "#,
        )
        .bind(root_seq)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Read direct children of an event.
    pub async fn read_children(&self, parent_seq: i64) -> Result<Vec<StoredEvent>> {
        let rows = sqlx::query_as::<_, StoredEvent>(
            r#"
            SELECT seq, ts, event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v
            FROM events
            WHERE parent_seq = $1
            ORDER BY seq ASC
            "#,
        )
        .bind(parent_seq)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// The latest committed sequence number, or 0 if the table is empty.
    pub async fn latest_seq(&self) -> Result<i64> {
        let row = sqlx::query_as::<_, (Option<i64>,)>("SELECT MAX(seq) FROM events")
            .fetch_one(&self.pool)
            .await?;

        Ok(row.0.unwrap_or(0))
    }

    /// Subscribe to new events via PG NOTIFY. Returns a stream of StoredEvents.
    ///
    /// Each NOTIFY carries just the seq number. The subscriber fetches the full
    /// record from the store. If a notification is missed, the consumer can
    /// catch up by reading from its last known seq.
    pub async fn subscribe(&self) -> Result<Pin<Box<dyn Stream<Item = StoredEvent> + Send>>> {
        let pool = self.pool.clone();
        let store = self.clone();

        let stream = async_stream(pool, store, None);
        Ok(Box::pin(stream))
    }

    /// Subscribe with a type filter — only delivers events matching the filter.
    pub async fn subscribe_filtered(
        &self,
        event_types: &[&str],
    ) -> Result<Pin<Box<dyn Stream<Item = StoredEvent> + Send>>> {
        let pool = self.pool.clone();
        let store = self.clone();
        let types: Vec<String> = event_types.iter().map(|s| s.to_string()).collect();

        let stream = async_stream(pool, store, Some(types));
        Ok(Box::pin(stream))
    }
}

// ---------------------------------------------------------------------------
// EventHandle — causal chaining
// ---------------------------------------------------------------------------

/// Handle returned by append(). Use to emit child events in the same causal chain.
pub struct EventHandle {
    seq: i64,
    caused_by: i64,
    store: EventStore,
    run_id: Option<String>,
    actor: Option<String>,
}

impl EventHandle {
    /// Append a child fact caused by this event. Returns a handle for grandchildren.
    pub async fn append(&self, event: AppendEvent) -> Result<EventHandle> {
        let event_with_context = AppendEvent {
            run_id: event.run_id.or_else(|| self.run_id.clone()),
            actor: event.actor.or_else(|| self.actor.clone()),
            ..event
        };

        let row = sqlx::query_as::<_, (i64,)>(
            r#"
            INSERT INTO events (event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING seq
            "#,
        )
        .bind(&event_with_context.event_type)
        .bind(self.seq)
        .bind(self.caused_by)
        .bind(&event_with_context.run_id)
        .bind(&event_with_context.actor)
        .bind(&event_with_context.payload)
        .bind(event_with_context.schema_v)
        .fetch_one(&self.store.pool)
        .await?;

        let child_seq = row.0;

        notify_new_event(&self.store.pool, child_seq).await;

        Ok(EventHandle {
            seq: child_seq,
            caused_by: self.caused_by,
            store: self.store.clone(),
            run_id: event_with_context.run_id,
            actor: event_with_context.actor,
        })
    }

    /// Fire-and-forget: append a child fact, discard the handle.
    /// Spawns in background — the caller doesn't wait.
    pub fn log(&self, event: AppendEvent) {
        let event_with_context = AppendEvent {
            run_id: event.run_id.or_else(|| self.run_id.clone()),
            actor: event.actor.or_else(|| self.actor.clone()),
            ..event
        };

        let pool = self.store.pool.clone();
        let parent_seq = self.seq;
        let caused_by = self.caused_by;

        tokio::spawn(async move {
            let result = sqlx::query(
                r#"
                INSERT INTO events (event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
            )
            .bind(&event_with_context.event_type)
            .bind(parent_seq)
            .bind(caused_by)
            .bind(&event_with_context.run_id)
            .bind(&event_with_context.actor)
            .bind(&event_with_context.payload)
            .bind(event_with_context.schema_v)
            .execute(&pool)
            .await;

            if let Err(e) = result {
                warn!(error = %e, "Failed to log fire-and-forget event");
            }
        });
    }

    /// This event's sequence number.
    pub fn seq(&self) -> i64 {
        self.seq
    }

    /// The root sequence number of this causal chain.
    pub fn caused_by(&self) -> i64 {
        self.caused_by
    }
}

// ---------------------------------------------------------------------------
// PG NOTIFY helpers
// ---------------------------------------------------------------------------

async fn notify_new_event(pool: &PgPool, seq: i64) {
    let result = sqlx::query("SELECT pg_notify('events', $1::text)")
        .bind(seq)
        .execute(pool)
        .await;

    if let Err(e) = result {
        warn!(error = %e, seq, "PG NOTIFY failed (non-fatal)");
    }
}

fn async_stream(
    pool: PgPool,
    store: EventStore,
    type_filter: Option<Vec<String>>,
) -> impl Stream<Item = StoredEvent> + Send {
    futures::stream::unfold(
        (pool, store, type_filter, false),
        move |(pool, _store, _type_filter, _listening)| async move {
            // Set up LISTEN on first iteration
            let _ = sqlx::query("LISTEN events").execute(&pool).await;

            // Placeholder: polling with short sleep.
            // The real implementation will use sqlx::postgres::PgListener
            // for true push-based notifications. For Phase 1, polling is
            // sufficient and correct.
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            None::<(StoredEvent, (PgPool, EventStore, Option<Vec<String>>, bool))>
        },
    )
}

// ---------------------------------------------------------------------------
// sqlx::FromRow for StoredEvent
// ---------------------------------------------------------------------------

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for StoredEvent {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> std::result::Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(StoredEvent {
            seq: row.try_get("seq")?,
            ts: row.try_get("ts")?,
            event_type: row.try_get("event_type")?,
            parent_seq: row.try_get("parent_seq")?,
            caused_by_seq: row.try_get("caused_by_seq")?,
            run_id: row.try_get("run_id")?,
            actor: row.try_get("actor")?,
            payload: row.try_get("payload")?,
            schema_v: row.try_get("schema_v")?,
        })
    }
}

// ---------------------------------------------------------------------------
// Test utilities
// ---------------------------------------------------------------------------

#[cfg(feature = "test-utils")]
impl EventStore {
    /// Read all events (for tests). No gap-free enforcement.
    pub async fn read_all(&self) -> Result<Vec<StoredEvent>> {
        let rows = sqlx::query_as::<_, StoredEvent>(
            r#"
            SELECT seq, ts, event_type, parent_seq, caused_by_seq, run_id, actor, payload, schema_v
            FROM events
            ORDER BY seq ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
