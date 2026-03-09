//! PostgresStore — durable seesaw store backed by Postgres.
//!
//! Scoped by `correlation_id`. Implements seesaw 0.26's split traits:
//! `EventLog` (append-only event persistence) and `HandlerQueue`
//! (checkpoint-based work distribution with journaling).

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

use seesaw_core::event_log::EventLog;
use seesaw_core::handler_queue::HandlerQueue;
use seesaw_core::types::{
    AppendResult, HandlerCompletion, HandlerDlq, HandlerResolution, IntentCommit, JournalEntry,
    LogLevel, QueueStatus, QueuedHandler,
};
use seesaw_core::{NewEvent, PersistedEvent, Snapshot};

/// Postgres-backed seesaw store, scoped by a single correlation_id.
pub struct PostgresStore {
    pool: PgPool,
    correlation_id: Uuid,
}

impl PostgresStore {
    pub fn new(pool: PgPool, correlation_id: Uuid) -> Self {
        Self {
            pool,
            correlation_id,
        }
    }

    /// Check if this correlation has any pending handler work.
    pub async fn has_pending_work(&self) -> Result<bool> {
        let row = sqlx::query_as::<_, (bool,)>(
            "SELECT EXISTS( \
                SELECT 1 FROM seesaw_effect_executions \
                WHERE correlation_id = $1 AND status IN ('pending', 'running') \
             )",
        )
        .bind(self.correlation_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }
}

// ── EventLog ────────────────────────────────────────────────────────────

#[async_trait]
impl EventLog for PostgresStore {
    async fn append(&self, event: NewEvent) -> Result<AppendResult> {
        let run_id = event
            .metadata
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let schema_v = event
            .metadata
            .get("schema_v")
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as i16;
        let handler_id = event
            .metadata
            .get("handler_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let result = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO events \
             (event_type, run_id, payload, schema_v, id, parent_id, correlation_id, \
              aggregate_type, aggregate_id, handler_id, persistent) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (id) WHERE id IS NOT NULL DO NOTHING \
             RETURNING seq",
        )
        .bind(&event.event_type)
        .bind(&run_id)
        .bind(&event.payload)
        .bind(schema_v)
        .bind(event.event_id)
        .bind(event.parent_id)
        .bind(event.correlation_id)
        .bind(&event.aggregate_type)
        .bind(event.aggregate_id)
        .bind(&handler_id)
        .bind(event.persistent)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some((seq,)) => {
                let _ = sqlx::query("SELECT pg_notify('events', $1::text)")
                    .bind(seq)
                    .execute(&self.pool)
                    .await;
                Ok(AppendResult {
                    position: seq as u64,
                    version: None,
                })
            }
            None => {
                let (seq,) =
                    sqlx::query_as::<_, (i64,)>("SELECT seq FROM events WHERE id = $1")
                        .bind(event.event_id)
                        .fetch_one(&self.pool)
                        .await?;
                Ok(AppendResult {
                    position: seq as u64,
                    version: None,
                })
            }
        }
    }

    async fn load_from(
        &self,
        after_position: u64,
        limit: usize,
    ) -> Result<Vec<PersistedEvent>> {
        let rows = sqlx::query_as::<_, PersistedEventRow>(
            "SELECT seq, id, parent_id, correlation_id, event_type, payload, ts, \
                    aggregate_type, aggregate_id, run_id, schema_v, handler_id, persistent \
             FROM events \
             WHERE seq > $1 \
             ORDER BY seq ASC \
             LIMIT $2",
        )
        .bind(after_position as i64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_persisted()).collect())
    }

    async fn load_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: Uuid,
        after_version: Option<u64>,
    ) -> Result<Vec<PersistedEvent>> {
        let rows = match after_version {
            Some(pos) => {
                sqlx::query_as::<_, PersistedEventRow>(
                    "SELECT seq, id, parent_id, correlation_id, event_type, payload, ts, \
                            aggregate_type, aggregate_id, run_id, schema_v, handler_id, persistent \
                     FROM events \
                     WHERE aggregate_type = $1 AND aggregate_id = $2 \
                       AND correlation_id = $3 AND seq > $4 \
                     ORDER BY seq ASC",
                )
                .bind(aggregate_type)
                .bind(aggregate_id)
                .bind(self.correlation_id)
                .bind(pos as i64)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, PersistedEventRow>(
                    "SELECT seq, id, parent_id, correlation_id, event_type, payload, ts, \
                            aggregate_type, aggregate_id, run_id, schema_v, handler_id, persistent \
                     FROM events \
                     WHERE aggregate_type = $1 AND aggregate_id = $2 \
                       AND correlation_id = $3 \
                     ORDER BY seq ASC",
                )
                .bind(aggregate_type)
                .bind(aggregate_id)
                .bind(self.correlation_id)
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(rows.into_iter().map(|r| r.into_persisted()).collect())
    }

    async fn load_snapshot(
        &self,
        aggregate_type: &str,
        aggregate_id: Uuid,
    ) -> Result<Option<Snapshot>> {
        let row = sqlx::query_as::<_, SnapshotRow>(
            "SELECT aggregate_type, aggregate_id, version, state, created_at \
             FROM aggregate_snapshots \
             WHERE aggregate_type = $1 AND aggregate_id = $2 AND correlation_id = $3",
        )
        .bind(aggregate_type)
        .bind(aggregate_id)
        .bind(self.correlation_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Snapshot {
            aggregate_type: r.aggregate_type,
            aggregate_id: r.aggregate_id,
            version: r.version as u64,
            state: r.state,
            created_at: r.created_at,
        }))
    }

    async fn save_snapshot(&self, snapshot: Snapshot) -> Result<()> {
        sqlx::query(
            "INSERT INTO aggregate_snapshots (aggregate_type, aggregate_id, correlation_id, version, state, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (aggregate_type, aggregate_id, correlation_id) \
             DO UPDATE SET version = EXCLUDED.version, state = EXCLUDED.state, created_at = EXCLUDED.created_at",
        )
        .bind(&snapshot.aggregate_type)
        .bind(snapshot.aggregate_id)
        .bind(self.correlation_id)
        .bind(snapshot.version as i64)
        .bind(&snapshot.state)
        .bind(snapshot.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// ── HandlerQueue ────────────────────────────────────────────────────────

#[async_trait]
impl HandlerQueue for PostgresStore {
    async fn enqueue(&self, commit: IntentCommit) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Advance checkpoint
        sqlx::query(
            "INSERT INTO seesaw_checkpoints (correlation_id, position, updated_at) \
             VALUES ($1, $2, now()) \
             ON CONFLICT (correlation_id) \
             DO UPDATE SET position = EXCLUDED.position, updated_at = now()",
        )
        .bind(commit.correlation_id)
        .bind(commit.checkpoint as i64)
        .execute(&mut *tx)
        .await?;

        // Insert handler intents
        for intent in &commit.intents {
            sqlx::query(
                "INSERT INTO seesaw_effect_executions \
                 (event_id, handler_id, correlation_id, event_type, event_payload, \
                  parent_event_id, hops, \
                  max_attempts, timeout_seconds, priority, execute_at, status) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending')",
            )
            .bind(commit.event_id)
            .bind(&intent.handler_id)
            .bind(commit.correlation_id)
            .bind(&commit.event_type)
            .bind(&commit.event_payload)
            .bind(intent.parent_event_id)
            .bind(intent.hops)
            .bind(intent.max_attempts)
            .bind(intent.timeout_seconds)
            .bind(intent.priority)
            .bind(intent.execute_at)
            .execute(&mut *tx)
            .await?;
        }

        // DLQ projection failures
        for failure in &commit.projection_failures {
            sqlx::query(
                "INSERT INTO seesaw_dead_letter_queue \
                 (event_id, handler_id, error, reason, attempts, payload) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(commit.event_id)
            .bind(&failure.handler_id)
            .bind(&failure.error)
            .bind(&failure.reason)
            .bind(failure.attempts)
            .bind(&commit.event_payload)
            .execute(&mut *tx)
            .await?;
        }

        // Park event if requested
        if let Some(park) = &commit.park {
            sqlx::query(
                "INSERT INTO seesaw_dead_letter_queue \
                 (event_id, error, reason, payload) \
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(commit.event_id)
            .bind(&park.reason)
            .bind("parked")
            .bind(&commit.event_payload)
            .execute(&mut *tx)
            .await?;
        }

        // Handler descriptions
        for (handler_id, data) in &commit.handler_descriptions {
            sqlx::query(
                "INSERT INTO seesaw_handler_descriptions \
                 (correlation_id, handler_id, description, updated_at) \
                 VALUES ($1, $2, $3, now()) \
                 ON CONFLICT (correlation_id, handler_id) \
                 DO UPDATE SET description = EXCLUDED.description, updated_at = now()",
            )
            .bind(commit.correlation_id)
            .bind(handler_id)
            .bind(data)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn checkpoint(&self) -> Result<u64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT position FROM seesaw_checkpoints WHERE correlation_id = $1",
        )
        .bind(self.correlation_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(pos,)| pos as u64).unwrap_or(0))
    }

    async fn dequeue(&self) -> Result<Option<QueuedHandler>> {
        let row = sqlx::query_as::<_, EffectRow>(
            "UPDATE seesaw_effect_executions SET status = 'running', updated_at = now() \
             WHERE (event_id, handler_id) = ( \
                 SELECT event_id, handler_id FROM seesaw_effect_executions \
                 WHERE correlation_id = $1 AND status = 'pending' AND execute_at <= now() \
                 ORDER BY priority ASC, execute_at ASC \
                 FOR UPDATE SKIP LOCKED \
                 LIMIT 1 \
             ) \
             RETURNING event_id, handler_id, correlation_id, event_type, event_payload, \
                       parent_event_id, \
                       execute_at, timeout_seconds, max_attempts, priority, hops, attempts",
        )
        .bind(self.correlation_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.into_queued()))
    }

    async fn earliest_pending_at(&self) -> Result<Option<DateTime<Utc>>> {
        let row = sqlx::query_as::<_, (Option<DateTime<Utc>>,)>(
            "SELECT MIN(execute_at) FROM seesaw_effect_executions \
             WHERE correlation_id = $1 AND status = 'pending'",
        )
        .bind(self.correlation_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn resolve(&self, resolution: HandlerResolution) -> Result<()> {
        match resolution {
            HandlerResolution::Complete(completion) => {
                self.complete_handler_inner(completion).await
            }
            HandlerResolution::Retry {
                event_id,
                handler_id,
                error,
                new_attempts,
                next_execute_at,
            } => {
                sqlx::query(
                    "UPDATE seesaw_effect_executions \
                     SET status = 'pending', attempts = $3, execute_at = $4, error = $5, updated_at = now() \
                     WHERE event_id = $1 AND handler_id = $2 AND correlation_id = $6",
                )
                .bind(event_id)
                .bind(&handler_id)
                .bind(new_attempts)
                .bind(next_execute_at)
                .bind(&error)
                .bind(self.correlation_id)
                .execute(&self.pool)
                .await?;
                Ok(())
            }
            HandlerResolution::DeadLetter(dlq) => self.dlq_handler_inner(dlq).await,
        }
    }

    async fn reclaim_stale(&self) -> Result<()> {
        sqlx::query(
            "UPDATE seesaw_effect_executions SET status = 'pending', updated_at = now() \
             WHERE correlation_id = $1 AND status = 'running'",
        )
        .bind(self.correlation_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Journaling ────────────────────────────────────────────────────

    async fn load_journal(
        &self,
        handler_id: &str,
        event_id: Uuid,
    ) -> Result<Vec<JournalEntry>> {
        let rows = sqlx::query_as::<_, (i32, serde_json::Value)>(
            "SELECT seq, value FROM seesaw_handler_journal \
             WHERE handler_id = $1 AND event_id = $2 \
             ORDER BY seq ASC",
        )
        .bind(handler_id)
        .bind(event_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(seq, value)| JournalEntry {
                seq: seq as u32,
                value,
            })
            .collect())
    }

    async fn append_journal(
        &self,
        handler_id: &str,
        event_id: Uuid,
        seq: u32,
        value: serde_json::Value,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO seesaw_handler_journal (handler_id, event_id, seq, value) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(handler_id)
        .bind(event_id)
        .bind(seq as i32)
        .bind(&value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn clear_journal(
        &self,
        handler_id: &str,
        event_id: Uuid,
    ) -> Result<()> {
        sqlx::query(
            "DELETE FROM seesaw_handler_journal \
             WHERE handler_id = $1 AND event_id = $2",
        )
        .bind(handler_id)
        .bind(event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Coordination ──────────────────────────────────────────────────

    async fn cancel(&self, correlation_id: Uuid) -> Result<()> {
        sqlx::query(
            "INSERT INTO seesaw_cancellations (correlation_id) \
             VALUES ($1) \
             ON CONFLICT DO NOTHING",
        )
        .bind(correlation_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn is_cancelled(&self, correlation_id: Uuid) -> Result<bool> {
        let (exists,) = sqlx::query_as::<_, (bool,)>(
            "SELECT EXISTS(SELECT 1 FROM seesaw_cancellations WHERE correlation_id = $1)",
        )
        .bind(correlation_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    async fn status(&self, correlation_id: Uuid) -> Result<QueueStatus> {
        let (pending_effects,) = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM seesaw_effect_executions \
             WHERE correlation_id = $1 AND status IN ('pending', 'running')",
        )
        .bind(correlation_id)
        .fetch_one(&self.pool)
        .await?;

        let (dead_lettered,) = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM seesaw_effect_executions \
             WHERE correlation_id = $1 AND status = 'dead_lettered'",
        )
        .bind(correlation_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(QueueStatus {
            pending_handlers: pending_effects as usize,
            dead_lettered: dead_lettered as usize,
        })
    }

    async fn set_descriptions(
        &self,
        correlation_id: Uuid,
        descriptions: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        for (handler_id, data) in descriptions {
            sqlx::query(
                "INSERT INTO seesaw_handler_descriptions \
                 (correlation_id, handler_id, description, updated_at) \
                 VALUES ($1, $2, $3, now()) \
                 ON CONFLICT (correlation_id, handler_id) \
                 DO UPDATE SET description = EXCLUDED.description, updated_at = now()",
            )
            .bind(correlation_id)
            .bind(&handler_id)
            .bind(&data)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn get_descriptions(
        &self,
        correlation_id: Uuid,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
            "SELECT handler_id, description FROM seesaw_handler_descriptions \
             WHERE correlation_id = $1",
        )
        .bind(correlation_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }
}

// ── Private helpers ─────────────────────────────────────────────────

impl PostgresStore {
    async fn complete_handler_inner(&self, completion: HandlerCompletion) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "UPDATE seesaw_effect_executions \
             SET status = 'completed', result = $3, updated_at = now() \
             WHERE event_id = $1 AND handler_id = $2 AND correlation_id = $4",
        )
        .bind(completion.event_id)
        .bind(&completion.handler_id)
        .bind(&completion.result)
        .bind(self.correlation_id)
        .execute(&mut *tx)
        .await?;

        for entry in &completion.log_entries {
            sqlx::query(
                "INSERT INTO seesaw_handler_logs \
                 (event_id, handler_id, correlation_id, level, message, data, logged_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(completion.event_id)
            .bind(&completion.handler_id)
            .bind(self.correlation_id)
            .bind(log_level_str(&entry.level))
            .bind(&entry.message)
            .bind(&entry.data)
            .bind(entry.timestamp)
            .execute(&mut *tx)
            .await?;
        }

        // Clear journal entries on successful completion
        sqlx::query(
            "DELETE FROM seesaw_handler_journal \
             WHERE handler_id = $1 AND event_id = $2",
        )
        .bind(&completion.handler_id)
        .bind(completion.event_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn dlq_handler_inner(&self, dlq: HandlerDlq) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "UPDATE seesaw_effect_executions \
             SET status = 'dead_lettered', error = $3, updated_at = now() \
             WHERE event_id = $1 AND handler_id = $2 AND correlation_id = $4",
        )
        .bind(dlq.event_id)
        .bind(&dlq.handler_id)
        .bind(&dlq.error)
        .bind(self.correlation_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO seesaw_dead_letter_queue \
             (event_id, handler_id, error, reason, attempts) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(dlq.event_id)
        .bind(&dlq.handler_id)
        .bind(&dlq.error)
        .bind(&dlq.reason)
        .bind(dlq.attempts)
        .execute(&mut *tx)
        .await?;

        for entry in &dlq.log_entries {
            sqlx::query(
                "INSERT INTO seesaw_handler_logs \
                 (event_id, handler_id, correlation_id, level, message, data, logged_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(dlq.event_id)
            .bind(&dlq.handler_id)
            .bind(self.correlation_id)
            .bind(log_level_str(&entry.level))
            .bind(&entry.message)
            .bind(&entry.data)
            .bind(entry.timestamp)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

fn log_level_str(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
    }
}

// ── Internal row types ───────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct EffectRow {
    event_id: Uuid,
    handler_id: String,
    correlation_id: Uuid,
    event_type: String,
    event_payload: serde_json::Value,
    parent_event_id: Option<Uuid>,
    execute_at: DateTime<Utc>,
    timeout_seconds: i32,
    max_attempts: i32,
    priority: i32,
    hops: i32,
    attempts: i32,
}

impl EffectRow {
    fn into_queued(self) -> QueuedHandler {
        QueuedHandler {
            event_id: self.event_id,
            handler_id: self.handler_id,
            correlation_id: self.correlation_id,
            event_type: self.event_type,
            event_payload: self.event_payload,
            parent_event_id: self.parent_event_id,
            execute_at: self.execute_at,
            timeout_seconds: self.timeout_seconds,
            max_attempts: self.max_attempts,
            priority: self.priority,
            hops: self.hops,
            attempts: self.attempts,
            ephemeral: None,
        }
    }
}

#[derive(sqlx::FromRow)]
struct PersistedEventRow {
    seq: i64,
    id: Option<Uuid>,
    parent_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
    event_type: String,
    payload: serde_json::Value,
    ts: DateTime<Utc>,
    aggregate_type: Option<String>,
    aggregate_id: Option<Uuid>,
    run_id: Option<String>,
    schema_v: i16,
    handler_id: Option<String>,
    persistent: bool,
}

impl PersistedEventRow {
    fn into_persisted(self) -> PersistedEvent {
        let mut metadata = serde_json::Map::new();
        if let Some(run_id) = self.run_id {
            metadata.insert("run_id".to_string(), serde_json::Value::String(run_id));
        }
        metadata.insert("schema_v".to_string(), serde_json::json!(self.schema_v));
        if let Some(handler_id) = self.handler_id {
            metadata.insert(
                "handler_id".to_string(),
                serde_json::Value::String(handler_id),
            );
        }

        PersistedEvent {
            position: self.seq as u64,
            event_id: self.id.unwrap_or_else(Uuid::new_v4),
            parent_id: self.parent_id,
            correlation_id: self.correlation_id.unwrap_or_else(Uuid::new_v4),
            event_type: self.event_type,
            payload: self.payload,
            created_at: self.ts,
            aggregate_type: self.aggregate_type,
            aggregate_id: self.aggregate_id,
            version: None,
            metadata,
            ephemeral: None,
            persistent: self.persistent,
        }
    }
}

#[derive(sqlx::FromRow)]
struct SnapshotRow {
    aggregate_type: String,
    aggregate_id: Uuid,
    version: i64,
    state: serde_json::Value,
    created_at: DateTime<Utc>,
}
