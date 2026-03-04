//! PostgresStore — durable seesaw Store backed by Postgres.
//!
//! Scoped by `correlation_id`. Implements seesaw 0.20's unified `Store` trait
//! covering event queue, effect queue, joins, event persistence, and snapshots.
//!
//! Queue tables: `seesaw_events`, `seesaw_effect_executions`, `seesaw_join_*`,
//! `seesaw_dead_letter_queue`.
//!
//! Event persistence reuses the existing `events` table with ON CONFLICT for
//! idempotent append. Snapshots reuse `aggregate_snapshots`.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use seesaw_core::store::Store;
use seesaw_core::types::{EffectResolution, EventOutcome, QueueStatus};
use seesaw_core::{
    EffectCompletion, EffectDlq, EventCommit, ExpiredJoinWindow, JoinAppendParams, JoinEntry,
    NewEvent, PersistedEvent, QueuedEffect, QueuedEvent, Snapshot,
};

/// Postgres-backed seesaw Store, scoped by a single correlation_id.
///
/// All queue operations are filtered by `correlation_id`, so multiple engines
/// (each with a different run) can share the same tables without interference.
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

    /// Check if this correlation has any pending work (events or effects).
    pub async fn has_pending_work(&self) -> Result<bool> {
        let row = sqlx::query_as::<_, (bool,)>(
            "SELECT EXISTS( \
                SELECT 1 FROM seesaw_events \
                WHERE correlation_id = $1 AND status IN ('pending', 'processing') \
             ) OR EXISTS( \
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

#[async_trait]
impl Store for PostgresStore {
    // ── Event queue ──────────────────────────────────────────────────

    async fn publish(&self, event: QueuedEvent) -> Result<()> {
        sqlx::query(
            "INSERT INTO seesaw_events \
             (event_id, parent_id, correlation_id, event_type, payload, handler_id, \
              hops, retry_count, batch_id, batch_index, batch_size, status, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending', $12)",
        )
        .bind(event.event_id)
        .bind(event.parent_id)
        .bind(self.correlation_id)
        .bind(&event.event_type)
        .bind(&event.payload)
        .bind(&event.handler_id)
        .bind(event.hops)
        .bind(event.retry_count)
        .bind(event.batch_id)
        .bind(event.batch_index)
        .bind(event.batch_size)
        .bind(event.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn poll_next(&self) -> Result<Option<QueuedEvent>> {
        let row = sqlx::query_as::<_, QueuedEventRow>(
            "UPDATE seesaw_events SET status = 'processing' \
             WHERE row_id = ( \
                 SELECT row_id FROM seesaw_events \
                 WHERE correlation_id = $1 AND status = 'pending' \
                 ORDER BY row_id ASC \
                 FOR UPDATE SKIP LOCKED \
                 LIMIT 1 \
             ) \
             RETURNING row_id, event_id, parent_id, correlation_id, event_type, payload, \
                       handler_id, hops, retry_count, batch_id, batch_index, batch_size, created_at",
        )
        .bind(self.correlation_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.into_queued_event()))
    }

    async fn complete_event(&self, result: EventOutcome) -> Result<()> {
        match result {
            EventOutcome::Processed(commit) => self.commit_event(commit).await,
            EventOutcome::Rejected {
                event_row_id,
                event_id,
                error,
                reason,
            } => {
                let mut tx = self.pool.begin().await?;

                sqlx::query(
                    "UPDATE seesaw_events SET status = 'rejected' \
                     WHERE row_id = $1 AND correlation_id = $2",
                )
                .bind(event_row_id)
                .bind(self.correlation_id)
                .execute(&mut *tx)
                .await?;

                sqlx::query(
                    "INSERT INTO seesaw_dead_letter_queue \
                     (event_id, handler_id, error, reason) \
                     VALUES ($1, NULL, $2, $3)",
                )
                .bind(event_id)
                .bind(&error)
                .bind(&reason)
                .execute(&mut *tx)
                .await?;

                tx.commit().await?;
                Ok(())
            }
        }
    }

    // ── Effect queue ─────────────────────────────────────────────────

    async fn poll_next_effect(&self) -> Result<Option<QueuedEffect>> {
        let row = sqlx::query_as::<_, EffectRow>(
            "UPDATE seesaw_effect_executions SET status = 'running', updated_at = now() \
             WHERE (event_id, handler_id) = ( \
                 SELECT event_id, handler_id FROM seesaw_effect_executions \
                 WHERE correlation_id = $1 AND status = 'pending' AND execute_at <= now() \
                 ORDER BY priority DESC, execute_at ASC \
                 FOR UPDATE SKIP LOCKED \
                 LIMIT 1 \
             ) \
             RETURNING event_id, handler_id, correlation_id, event_type, event_payload, \
                       parent_event_id, batch_id, batch_index, batch_size, \
                       execute_at, timeout_seconds, max_attempts, priority, hops, attempts, \
                       join_window_timeout_seconds",
        )
        .bind(self.correlation_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.into_queued()))
    }

    async fn earliest_pending_effect_at(&self) -> Result<Option<DateTime<Utc>>> {
        let row = sqlx::query_as::<_, (Option<DateTime<Utc>>,)>(
            "SELECT MIN(execute_at) FROM seesaw_effect_executions \
             WHERE correlation_id = $1 AND status = 'pending'",
        )
        .bind(self.correlation_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn resolve_effect(&self, resolution: EffectResolution) -> Result<()> {
        match resolution {
            EffectResolution::Complete(completion) => {
                self.complete_effect_inner(completion).await
            }
            EffectResolution::Retry {
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
            EffectResolution::DeadLetter(dlq) => self.dlq_effect_inner(dlq).await,
        }
    }

    async fn reclaim_stale(&self) -> Result<()> {
        // Reset stale `processing` events back to `pending`
        sqlx::query(
            "UPDATE seesaw_events SET status = 'pending' \
             WHERE correlation_id = $1 AND status = 'processing'",
        )
        .bind(self.correlation_id)
        .execute(&self.pool)
        .await?;

        // Reset stale `running` effects back to `pending`
        sqlx::query(
            "UPDATE seesaw_effect_executions SET status = 'pending', updated_at = now() \
             WHERE correlation_id = $1 AND status = 'running'",
        )
        .bind(self.correlation_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ── Join windows ─────────────────────────────────────────────────

    async fn join_append_and_maybe_claim(
        &self,
        params: JoinAppendParams,
    ) -> Result<Option<Vec<JoinEntry>>> {
        let mut tx = self.pool.begin().await?;

        // Upsert window
        let timeout_at = params
            .join_window_timeout_seconds
            .map(|secs| params.source_created_at + chrono::Duration::seconds(secs as i64));

        sqlx::query(
            "INSERT INTO seesaw_join_windows \
             (join_handler_id, correlation_id, batch_id, batch_size, timeout_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (join_handler_id, correlation_id, batch_id) DO NOTHING",
        )
        .bind(&params.join_handler_id)
        .bind(self.correlation_id)
        .bind(params.batch_id)
        .bind(params.batch_size)
        .bind(timeout_at)
        .execute(&mut *tx)
        .await?;

        // Insert entry
        sqlx::query(
            "INSERT INTO seesaw_join_entries \
             (join_handler_id, correlation_id, batch_id, batch_index, \
              source_event_id, event_type, payload, batch_size, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             ON CONFLICT (join_handler_id, correlation_id, batch_id, batch_index) DO NOTHING",
        )
        .bind(&params.join_handler_id)
        .bind(self.correlation_id)
        .bind(params.batch_id)
        .bind(params.batch_index)
        .bind(params.source_event_id)
        .bind(&params.source_event_type)
        .bind(&params.source_payload)
        .bind(params.batch_size)
        .bind(params.source_created_at)
        .execute(&mut *tx)
        .await?;

        // Count entries
        let (count,) = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM seesaw_join_entries \
             WHERE join_handler_id = $1 AND correlation_id = $2 AND batch_id = $3",
        )
        .bind(&params.join_handler_id)
        .bind(self.correlation_id)
        .bind(params.batch_id)
        .fetch_one(&mut *tx)
        .await?;

        if count < params.batch_size as i64 {
            tx.commit().await?;
            return Ok(None);
        }

        // All entries present — claim the window
        let updated = sqlx::query(
            "UPDATE seesaw_join_windows SET status = 'claimed' \
             WHERE join_handler_id = $1 AND correlation_id = $2 AND batch_id = $3 \
               AND status = 'open'",
        )
        .bind(&params.join_handler_id)
        .bind(self.correlation_id)
        .bind(params.batch_id)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            // Already claimed by another worker
            tx.commit().await?;
            return Ok(None);
        }

        let entries = sqlx::query_as::<_, JoinEntryRow>(
            "SELECT source_event_id, event_type, payload, batch_id, batch_index, batch_size, created_at \
             FROM seesaw_join_entries \
             WHERE join_handler_id = $1 AND correlation_id = $2 AND batch_id = $3 \
             ORDER BY batch_index ASC",
        )
        .bind(&params.join_handler_id)
        .bind(self.correlation_id)
        .bind(params.batch_id)
        .fetch_all(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some(entries.into_iter().map(|r| r.into_entry()).collect()))
    }

    async fn join_complete(
        &self,
        join_handler_id: String,
        correlation_id: Uuid,
        batch_id: Uuid,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE seesaw_join_windows SET status = 'completed' \
             WHERE join_handler_id = $1 AND correlation_id = $2 AND batch_id = $3",
        )
        .bind(&join_handler_id)
        .bind(correlation_id)
        .bind(batch_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn join_release(
        &self,
        join_handler_id: String,
        correlation_id: Uuid,
        batch_id: Uuid,
        _error: String,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE seesaw_join_windows SET status = 'open' \
             WHERE join_handler_id = $1 AND correlation_id = $2 AND batch_id = $3",
        )
        .bind(&join_handler_id)
        .bind(correlation_id)
        .bind(batch_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn expire_join_windows(&self, now: DateTime<Utc>) -> Result<Vec<ExpiredJoinWindow>> {
        let mut tx = self.pool.begin().await?;

        // Find open windows past their timeout
        let windows = sqlx::query_as::<_, JoinWindowRow>(
            "SELECT join_handler_id, correlation_id, batch_id \
             FROM seesaw_join_windows \
             WHERE status = 'open' AND timeout_at IS NOT NULL AND timeout_at <= $1 \
             FOR UPDATE SKIP LOCKED",
        )
        .bind(now)
        .fetch_all(&mut *tx)
        .await?;

        let mut expired = Vec::new();

        for w in &windows {
            sqlx::query(
                "UPDATE seesaw_join_windows SET status = 'expired' \
                 WHERE join_handler_id = $1 AND correlation_id = $2 AND batch_id = $3",
            )
            .bind(&w.join_handler_id)
            .bind(w.correlation_id)
            .bind(w.batch_id)
            .execute(&mut *tx)
            .await?;

            let entry_ids = sqlx::query_as::<_, (Uuid,)>(
                "SELECT source_event_id FROM seesaw_join_entries \
                 WHERE join_handler_id = $1 AND correlation_id = $2 AND batch_id = $3",
            )
            .bind(&w.join_handler_id)
            .bind(w.correlation_id)
            .bind(w.batch_id)
            .fetch_all(&mut *tx)
            .await?;

            expired.push(ExpiredJoinWindow {
                join_handler_id: w.join_handler_id.clone(),
                correlation_id: w.correlation_id,
                batch_id: w.batch_id,
                source_event_ids: entry_ids.into_iter().map(|(id,)| id).collect(),
            });
        }

        tx.commit().await?;
        Ok(expired)
    }

    // ── Event persistence (existing `events` table) ──────────────────

    async fn append_event(&self, event: NewEvent) -> Result<u64> {
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

        // Idempotent append: ON CONFLICT returns nothing, so we query after.
        let result = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO events \
             (event_type, run_id, payload, schema_v, id, parent_id, correlation_id, \
              aggregate_type, aggregate_id, handler_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
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
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some((seq,)) => {
                // Best-effort PG NOTIFY
                let _ = sqlx::query("SELECT pg_notify('events', $1::text)")
                    .bind(seq)
                    .execute(&self.pool)
                    .await;
                Ok(seq as u64)
            }
            None => {
                // Duplicate event_id — return existing position
                let (seq,) = sqlx::query_as::<_, (i64,)>(
                    "SELECT seq FROM events WHERE id = $1",
                )
                .bind(event.event_id)
                .fetch_one(&self.pool)
                .await?;
                Ok(seq as u64)
            }
        }
    }

    async fn load_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: Uuid,
        after_position: Option<u64>,
    ) -> Result<Vec<PersistedEvent>> {
        let rows = match after_position {
            Some(pos) => {
                sqlx::query_as::<_, PersistedEventRow>(
                    "SELECT seq, id, parent_id, correlation_id, event_type, payload, ts, \
                            aggregate_type, aggregate_id, run_id, schema_v, handler_id \
                     FROM events \
                     WHERE aggregate_type = $1 AND aggregate_id = $2 AND seq > $3 \
                     ORDER BY seq ASC",
                )
                .bind(aggregate_type)
                .bind(aggregate_id)
                .bind(pos as i64)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, PersistedEventRow>(
                    "SELECT seq, id, parent_id, correlation_id, event_type, payload, ts, \
                            aggregate_type, aggregate_id, run_id, schema_v, handler_id \
                     FROM events \
                     WHERE aggregate_type = $1 AND aggregate_id = $2 \
                     ORDER BY seq ASC",
                )
                .bind(aggregate_type)
                .bind(aggregate_id)
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(rows.into_iter().map(|r| r.into_persisted()).collect())
    }

    async fn load_global_from(
        &self,
        after_position: u64,
        limit: usize,
    ) -> Result<Vec<PersistedEvent>> {
        let rows = sqlx::query_as::<_, PersistedEventRow>(
            "SELECT seq, id, parent_id, correlation_id, event_type, payload, ts, \
                    aggregate_type, aggregate_id, run_id, schema_v, handler_id \
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

    // ── Snapshots (existing `aggregate_snapshots` table) ─────────────

    async fn load_snapshot(
        &self,
        aggregate_type: &str,
        aggregate_id: Uuid,
    ) -> Result<Option<Snapshot>> {
        let row = sqlx::query_as::<_, SnapshotRow>(
            "SELECT aggregate_type, aggregate_id, version, state, created_at \
             FROM aggregate_snapshots \
             WHERE aggregate_type = $1 AND aggregate_id = $2",
        )
        .bind(aggregate_type)
        .bind(aggregate_id)
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
            "INSERT INTO aggregate_snapshots (aggregate_type, aggregate_id, version, state, created_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (aggregate_type, aggregate_id) \
             DO UPDATE SET version = EXCLUDED.version, state = EXCLUDED.state, created_at = EXCLUDED.created_at",
        )
        .bind(&snapshot.aggregate_type)
        .bind(snapshot.aggregate_id)
        .bind(snapshot.version as i64)
        .bind(&snapshot.state)
        .bind(snapshot.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Cancellation ──────────────────────────────────────────────────

    async fn cancel_correlation(&self, correlation_id: Uuid) -> Result<()> {
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

    // ── Queue status ─────────────────────────────────────────────────

    async fn queue_status(&self, correlation_id: Uuid) -> Result<QueueStatus> {
        let (pending_events,) = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM seesaw_events \
             WHERE correlation_id = $1 AND status IN ('pending', 'processing')",
        )
        .bind(correlation_id)
        .fetch_one(&self.pool)
        .await?;

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
            pending_events: pending_events as usize,
            pending_effects: pending_effects as usize,
            dead_lettered: dead_lettered as usize,
        })
    }
}

// ── Private helpers ─────────────────────────────────────────────────

impl PostgresStore {
    async fn commit_event(&self, commit: EventCommit) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // 1. Ack the source event
        sqlx::query(
            "UPDATE seesaw_events SET status = 'done' WHERE row_id = $1 AND correlation_id = $2",
        )
        .bind(commit.event_row_id)
        .bind(self.correlation_id)
        .execute(&mut *tx)
        .await?;

        // 2. Insert effect intents
        for intent in &commit.queued_effect_intents {
            sqlx::query(
                "INSERT INTO seesaw_effect_executions \
                 (event_id, handler_id, correlation_id, event_type, event_payload, \
                  parent_event_id, batch_id, batch_index, batch_size, hops, \
                  max_attempts, timeout_seconds, priority, execute_at, \
                  join_window_timeout_seconds, status) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, 'pending')",
            )
            .bind(commit.event_id)
            .bind(&intent.handler_id)
            .bind(self.correlation_id)
            .bind(&commit.event_type)
            .bind(&commit.event_payload)
            .bind(intent.parent_event_id)
            .bind(intent.batch_id)
            .bind(intent.batch_index)
            .bind(intent.batch_size)
            .bind(intent.hops)
            .bind(intent.max_attempts)
            .bind(intent.timeout_seconds)
            .bind(intent.priority)
            .bind(intent.execute_at)
            .bind(intent.join_window_timeout_seconds)
            .execute(&mut *tx)
            .await?;
        }

        // 3. Publish inline emitted events
        for evt in &commit.emitted_events {
            sqlx::query(
                "INSERT INTO seesaw_events \
                 (event_id, parent_id, correlation_id, event_type, payload, handler_id, \
                  hops, retry_count, batch_id, batch_index, batch_size, status, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending', $12)",
            )
            .bind(evt.event_id)
            .bind(evt.parent_id)
            .bind(self.correlation_id)
            .bind(&evt.event_type)
            .bind(&evt.payload)
            .bind(&evt.handler_id)
            .bind(evt.hops)
            .bind(evt.retry_count)
            .bind(evt.batch_id)
            .bind(evt.batch_index)
            .bind(evt.batch_size)
            .bind(evt.created_at)
            .execute(&mut *tx)
            .await?;
        }

        // 4. DLQ inline failures
        for failure in &commit.inline_effect_failures {
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

        tx.commit().await?;
        Ok(())
    }

    async fn complete_effect_inner(&self, completion: EffectCompletion) -> Result<()> {
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

        for evt in &completion.events_to_publish {
            sqlx::query(
                "INSERT INTO seesaw_events \
                 (event_id, parent_id, correlation_id, event_type, payload, handler_id, \
                  hops, retry_count, batch_id, batch_index, batch_size, status, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending', $12)",
            )
            .bind(evt.event_id)
            .bind(evt.parent_id)
            .bind(self.correlation_id)
            .bind(&evt.event_type)
            .bind(&evt.payload)
            .bind(&evt.handler_id)
            .bind(evt.hops)
            .bind(evt.retry_count)
            .bind(evt.batch_id)
            .bind(evt.batch_index)
            .bind(evt.batch_size)
            .bind(evt.created_at)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn dlq_effect_inner(&self, dlq: EffectDlq) -> Result<()> {
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

        for evt in &dlq.events_to_publish {
            sqlx::query(
                "INSERT INTO seesaw_events \
                 (event_id, parent_id, correlation_id, event_type, payload, handler_id, \
                  hops, retry_count, batch_id, batch_index, batch_size, status, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending', $12)",
            )
            .bind(evt.event_id)
            .bind(evt.parent_id)
            .bind(self.correlation_id)
            .bind(&evt.event_type)
            .bind(&evt.payload)
            .bind(&evt.handler_id)
            .bind(evt.hops)
            .bind(evt.retry_count)
            .bind(evt.batch_id)
            .bind(evt.batch_index)
            .bind(evt.batch_size)
            .bind(evt.created_at)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

// ── Internal row types ───────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct QueuedEventRow {
    row_id: i64,
    event_id: Uuid,
    parent_id: Option<Uuid>,
    correlation_id: Uuid,
    event_type: String,
    payload: serde_json::Value,
    handler_id: Option<String>,
    hops: i32,
    retry_count: i32,
    batch_id: Option<Uuid>,
    batch_index: Option<i32>,
    batch_size: Option<i32>,
    created_at: DateTime<Utc>,
}

impl QueuedEventRow {
    fn into_queued_event(self) -> QueuedEvent {
        QueuedEvent {
            id: self.row_id,
            event_id: self.event_id,
            parent_id: self.parent_id,
            correlation_id: self.correlation_id,
            event_type: self.event_type,
            payload: self.payload,
            handler_id: self.handler_id,
            hops: self.hops,
            retry_count: self.retry_count,
            batch_id: self.batch_id,
            batch_index: self.batch_index,
            batch_size: self.batch_size,
            created_at: self.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct EffectRow {
    event_id: Uuid,
    handler_id: String,
    correlation_id: Uuid,
    event_type: String,
    event_payload: serde_json::Value,
    parent_event_id: Option<Uuid>,
    batch_id: Option<Uuid>,
    batch_index: Option<i32>,
    batch_size: Option<i32>,
    execute_at: DateTime<Utc>,
    timeout_seconds: i32,
    max_attempts: i32,
    priority: i32,
    hops: i32,
    attempts: i32,
    join_window_timeout_seconds: Option<i32>,
}

impl EffectRow {
    fn into_queued(self) -> QueuedEffect {
        QueuedEffect {
            event_id: self.event_id,
            handler_id: self.handler_id,
            correlation_id: self.correlation_id,
            event_type: self.event_type,
            event_payload: self.event_payload,
            parent_event_id: self.parent_event_id,
            batch_id: self.batch_id,
            batch_index: self.batch_index,
            batch_size: self.batch_size,
            execute_at: self.execute_at,
            timeout_seconds: self.timeout_seconds,
            max_attempts: self.max_attempts,
            priority: self.priority,
            hops: self.hops,
            attempts: self.attempts,
            join_window_timeout_seconds: self.join_window_timeout_seconds,
        }
    }
}

#[derive(sqlx::FromRow)]
struct JoinEntryRow {
    source_event_id: Uuid,
    event_type: String,
    payload: serde_json::Value,
    batch_id: Uuid,
    batch_index: i32,
    batch_size: i32,
    created_at: DateTime<Utc>,
}

impl JoinEntryRow {
    fn into_entry(self) -> JoinEntry {
        JoinEntry {
            source_event_id: self.source_event_id,
            event_type: self.event_type,
            payload: self.payload,
            batch_id: self.batch_id,
            batch_index: self.batch_index,
            batch_size: self.batch_size,
            created_at: self.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct JoinWindowRow {
    join_handler_id: String,
    correlation_id: Uuid,
    batch_id: Uuid,
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
