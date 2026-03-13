use anyhow::Result;
use async_trait::async_trait;
use causal_inspector::{
    AggregateStateSnapshotEntry, EventQuery, InspectorReadModel, ReactorDescriptionEntry,
    ReactorDescriptionSnapshotEntry, ReactorLogEntry, ReactorOutcomeEntry, StoredEvent,
};
use causal_inspector::read_model::{
    AggregateLifecycleEntry, CorrelationSummaryEntry, ReactorAttemptEntry, ReactorDependencyEntry,
};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct PgInspectorReadModel {
    pool: PgPool,
}

impl PgInspectorReadModel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_stored_event(row: &sqlx::postgres::PgRow) -> StoredEvent {
    StoredEvent {
        seq: row.get("seq"),
        ts: row.get("ts"),
        event_type: row.get("event_type"),
        payload: row.get("payload"),
        id: row.get("id"),
        parent_id: row.get("parent_id"),
        correlation_id: row.get("correlation_id"),
        reactor_id: row.get::<Option<String>, _>("handler_id"),
        aggregate_type: row.get("aggregate_type"),
        aggregate_id: row.get("aggregate_id"),
        stream_version: None,
    }
}

#[async_trait]
impl InspectorReadModel for PgInspectorReadModel {
    async fn list_events(&self, query: &EventQuery) -> Result<Vec<StoredEvent>> {
        let limit = (query.limit as i64).min(200);
        let rows = sqlx::query(
            r#"
            SELECT seq, ts, event_type, payload, id, parent_id,
                   correlation_id, handler_id, aggregate_type, aggregate_id
            FROM events
            WHERE ($1::bigint IS NULL OR seq < $1)
              AND ($2::timestamptz IS NULL OR ts >= $2)
              AND ($3::timestamptz IS NULL OR ts <= $3)
              AND ($4::text IS NULL
                   OR payload::text ILIKE '%' || $4 || '%'
                   OR event_type ILIKE '%' || $4 || '%')
              AND ($5::text IS NULL OR correlation_id::text = $5)
              AND ($6::text IS NULL
                   OR (aggregate_type || ':' || aggregate_id::text) = $6)
            ORDER BY seq DESC
            LIMIT $7
            "#,
        )
        .bind(query.cursor)
        .bind(query.from)
        .bind(query.to)
        .bind(&query.search)
        .bind(&query.correlation_id)
        .bind(&query.aggregate_key)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(row_to_stored_event).collect())
    }

    async fn get_event(&self, seq: i64) -> Result<Option<StoredEvent>> {
        let row = sqlx::query(
            r#"
            SELECT seq, ts, event_type, payload, id, parent_id,
                   correlation_id, handler_id, aggregate_type, aggregate_id
            FROM events
            WHERE seq = $1
            "#,
        )
        .bind(seq)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.as_ref().map(row_to_stored_event))
    }

    async fn causal_tree(&self, seq: i64) -> Result<(Vec<StoredEvent>, i64)> {
        let rows = sqlx::query(
            r#"
            SELECT e.seq, e.ts, e.event_type, e.payload, e.id, e.parent_id,
                   e.correlation_id, e.handler_id, e.aggregate_type, e.aggregate_id
            FROM events e
            WHERE e.correlation_id = (SELECT correlation_id FROM events WHERE seq = $1)
              AND e.correlation_id IS NOT NULL
            ORDER BY e.seq
            "#,
        )
        .bind(seq)
        .fetch_all(&self.pool)
        .await?;

        let root_seq = rows
            .iter()
            .find(|r| r.get::<Option<Uuid>, _>("parent_id").is_none())
            .map(|r| r.get::<i64, _>("seq"))
            .unwrap_or(seq);

        Ok((rows.iter().map(row_to_stored_event).collect(), root_seq))
    }

    async fn causal_flow(&self, correlation_id: &str) -> Result<Vec<StoredEvent>> {
        let cid: Uuid = correlation_id.parse()?;
        let rows = sqlx::query(
            r#"
            SELECT seq, ts, event_type, payload, id, parent_id,
                   correlation_id, handler_id, aggregate_type, aggregate_id
            FROM events
            WHERE correlation_id = $1
            ORDER BY seq ASC
            "#,
        )
        .bind(cid)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(row_to_stored_event).collect())
    }

    async fn events_from_seq(&self, start_seq: i64, limit: usize) -> Result<Vec<StoredEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT seq, ts, event_type, payload, id, parent_id,
                   correlation_id, handler_id, aggregate_type, aggregate_id
            FROM events
            WHERE seq >= $1
            ORDER BY seq ASC
            LIMIT $2
            "#,
        )
        .bind(start_seq)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(row_to_stored_event).collect())
    }

    async fn reactor_logs(
        &self,
        event_id: Uuid,
        reactor_id: &str,
    ) -> Result<Vec<ReactorLogEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT event_id, handler_id, level, message, data, logged_at
            FROM seesaw_handler_logs
            WHERE event_id = $1 AND handler_id = $2
            ORDER BY id
            "#,
        )
        .bind(event_id)
        .bind(reactor_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| ReactorLogEntry {
                event_id: r.get("event_id"),
                reactor_id: r.get("handler_id"),
                level: r.get("level"),
                message: r.get("message"),
                data: r.get("data"),
                logged_at: r.get("logged_at"),
            })
            .collect())
    }

    async fn reactor_logs_by_correlation(
        &self,
        correlation_id: &str,
    ) -> Result<Vec<ReactorLogEntry>> {
        let cid: Uuid = correlation_id.parse()?;
        let rows = sqlx::query(
            r#"
            SELECT event_id, handler_id, level, message, data, logged_at
            FROM seesaw_handler_logs
            WHERE correlation_id = $1
            ORDER BY logged_at, id
            "#,
        )
        .bind(cid)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| ReactorLogEntry {
                event_id: r.get("event_id"),
                reactor_id: r.get("handler_id"),
                level: r.get("level"),
                message: r.get("message"),
                data: r.get("data"),
                logged_at: r.get("logged_at"),
            })
            .collect())
    }

    async fn reactor_outcomes(&self, correlation_id: &str) -> Result<Vec<ReactorOutcomeEntry>> {
        let cid: Uuid = correlation_id.parse()?;
        let rows = sqlx::query(
            r#"
            SELECT handler_id, status, error, attempts, created_at, updated_at, event_id
            FROM seesaw_effect_executions
            WHERE correlation_id = $1
            ORDER BY created_at
            "#,
        )
        .bind(cid)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                let event_id: Uuid = r.get("event_id");
                ReactorOutcomeEntry {
                    reactor_id: r.get("handler_id"),
                    status: r.get("status"),
                    error: r.get("error"),
                    attempts: r.get::<i32, _>("attempts") as i64,
                    started_at: r.get("created_at"),
                    completed_at: r.get("updated_at"),
                    triggering_event_ids: vec![event_id.to_string()],
                }
            })
            .collect())
    }

    async fn reactor_attempt_history(
        &self,
        _correlation_id: &str,
    ) -> Result<Vec<ReactorAttemptEntry>> {
        Ok(vec![])
    }

    async fn reactor_descriptions(
        &self,
        _correlation_id: &str,
    ) -> Result<Vec<ReactorDescriptionEntry>> {
        Ok(vec![])
    }

    async fn reactor_description_snapshots(
        &self,
        _correlation_id: &str,
    ) -> Result<Vec<ReactorDescriptionSnapshotEntry>> {
        Ok(vec![])
    }

    async fn aggregate_state_timeline(
        &self,
        _correlation_id: &str,
    ) -> Result<Vec<AggregateStateSnapshotEntry>> {
        Ok(vec![])
    }

    async fn list_correlations(
        &self,
        search: Option<&str>,
        limit: usize,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<CorrelationSummaryEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT correlation_id,
                   COUNT(*) AS event_count,
                   MIN(ts) AS first_ts,
                   MAX(ts) AS last_ts,
                   (array_agg(event_type ORDER BY seq ASC))[1] AS root_event_type,
                   bool_or(event_type LIKE '%failed%' OR event_type LIKE '%error%') AS has_errors
            FROM events
            WHERE correlation_id IS NOT NULL
              AND ($1::text IS NULL
                   OR correlation_id::text ILIKE '%' || $1 || '%'
                   OR event_type ILIKE '%' || $1 || '%')
            GROUP BY correlation_id
            HAVING ($3::timestamptz IS NULL OR MIN(ts) >= $3)
            ORDER BY MAX(ts) DESC
            LIMIT $2
            "#,
        )
        .bind(search)
        .bind(limit as i64)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                let cid: Uuid = r.get("correlation_id");
                CorrelationSummaryEntry {
                    correlation_id: cid.to_string(),
                    event_count: r.get("event_count"),
                    first_ts: r.get("first_ts"),
                    last_ts: r.get("last_ts"),
                    root_event_type: r
                        .get::<Option<String>, _>("root_event_type")
                        .unwrap_or_default(),
                    has_errors: r.get::<Option<bool>, _>("has_errors").unwrap_or(false),
                }
            })
            .collect())
    }

    async fn reactor_dependencies(&self) -> Result<Vec<ReactorDependencyEntry>> {
        Ok(vec![])
    }

    async fn aggregate_lifecycle(
        &self,
        _aggregate_key: &str,
        _limit: usize,
    ) -> Result<Vec<AggregateLifecycleEntry>> {
        Ok(vec![])
    }

    async fn list_aggregate_keys(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT aggregate_type || ':' || aggregate_id::text AS key
            FROM events
            WHERE aggregate_type IS NOT NULL AND aggregate_id IS NOT NULL
            ORDER BY key
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| r.get("key")).collect())
    }
}
