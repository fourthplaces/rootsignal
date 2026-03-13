use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::scout_run::EventRowFull;
use crate::graphql::schema::AdminEvent;

const DEFAULT_CAPACITY: usize = 500_000;

/// Bounded in-memory cache of recent events for fast admin panel queries.
/// Stores pre-computed `AdminEvent` values with side-indexes for O(1) lookups.
pub struct EventCache {
    events: VecDeque<Arc<AdminEvent>>,
    by_seq: HashMap<i64, Arc<AdminEvent>>,
    by_correlation: HashMap<Uuid, Vec<i64>>,
    by_run: HashMap<String, Vec<i64>>,
    by_handler: HashMap<String, Vec<i64>>,
    capacity: usize,
}

impl EventCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity.min(DEFAULT_CAPACITY)),
            by_seq: HashMap::new(),
            by_correlation: HashMap::new(),
            by_run: HashMap::new(),
            by_handler: HashMap::new(),
            capacity,
        }
    }

    /// Hydrate from Postgres — loads the most recent N events.
    pub async fn hydrate(pool: &PgPool, capacity: usize) -> anyhow::Result<Self> {
        let start = std::time::Instant::now();

        let columns = "seq, ts, event_type, payload AS data, id, parent_id, run_id, correlation_id, parent_seq, handler_id";
        let query = format!(
            "SELECT {columns} FROM events ORDER BY seq DESC LIMIT $1"
        );

        let rows = sqlx::query(&query)
            .bind(capacity as i64)
            .fetch_all(pool)
            .await?;

        let mut cache = Self::new(capacity);

        // Rows come in DESC order; iterate in reverse to push oldest first.
        for row in rows.into_iter().rev() {
            let event_row = row_to_event_full(&row);
            let event = Arc::new(AdminEvent::from(event_row));
            cache.push_unchecked(event);
        }

        let elapsed = start.elapsed();
        info!(
            events = cache.events.len(),
            elapsed_ms = elapsed.as_millis(),
            "Event cache hydrated"
        );

        Ok(cache)
    }

    /// Push a new event into the cache. Evicts the oldest if at capacity.
    pub fn push(&mut self, event: Arc<AdminEvent>) {
        if self.events.len() >= self.capacity {
            self.evict_oldest();
        }
        self.push_unchecked(event);
    }

    /// Push without capacity check — used during hydration.
    fn push_unchecked(&mut self, event: Arc<AdminEvent>) {
        let seq = event.seq;

        // Update side indexes
        self.by_seq.insert(seq, Arc::clone(&event));

        if let Some(cid) = event.correlation_id.as_deref().and_then(|s| Uuid::parse_str(s).ok()) {
            let bucket = self.by_correlation.entry(cid).or_default();
            bucket.push(seq);
        }

        if let Some(ref run_id) = event.run_id {
            let bucket = self.by_run.entry(run_id.clone()).or_default();
            bucket.push(seq);
        }

        if let Some(ref handler_id) = event.handler_id {
            let bucket = self.by_handler.entry(handler_id.clone()).or_default();
            bucket.push(seq);
        }

        self.events.push_back(event);
    }

    /// Remove the oldest event and clean up all indexes.
    fn evict_oldest(&mut self) {
        let Some(evicted) = self.events.pop_front() else {
            return;
        };

        let seq = evicted.seq;
        self.by_seq.remove(&seq);

        if let Some(cid) = evicted.correlation_id.as_deref().and_then(|s| Uuid::parse_str(s).ok()) {
            if let Some(bucket) = self.by_correlation.get_mut(&cid) {
                if let Ok(pos) = bucket.binary_search(&seq) {
                    bucket.remove(pos);
                }
                if bucket.is_empty() {
                    self.by_correlation.remove(&cid);
                }
            }
        }

        if let Some(ref run_id) = evicted.run_id {
            if let Some(bucket) = self.by_run.get_mut(run_id) {
                if let Ok(pos) = bucket.binary_search(&seq) {
                    bucket.remove(pos);
                }
                if bucket.is_empty() {
                    self.by_run.remove(run_id);
                }
            }
        }

        if let Some(ref handler_id) = evicted.handler_id {
            if let Some(bucket) = self.by_handler.get_mut(handler_id) {
                if let Ok(pos) = bucket.binary_search(&seq) {
                    bucket.remove(pos);
                }
                if bucket.is_empty() {
                    self.by_handler.remove(handler_id);
                }
            }
        }
    }

    /// Get a single event by seq.
    pub fn get_by_seq(&self, seq: i64) -> Option<Arc<AdminEvent>> {
        self.by_seq.get(&seq).cloned()
    }

    /// Paginated reverse-chronological event listing with optional filters.
    /// Returns (events, next_cursor).
    pub fn search(
        &self,
        term: Option<&str>,
        cursor: Option<i64>,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        run_id: Option<&str>,
        limit: usize,
    ) -> (Vec<Arc<AdminEvent>>, Option<i64>) {
        let term_lower = term.map(|t| t.to_lowercase());

        // If run_id filter is set and we have an index, use it for faster filtering
        let run_seqs: Option<&Vec<i64>> = run_id.and_then(|rid| self.by_run.get(rid));

        let mut results = Vec::with_capacity(limit);

        // Iterate from newest (back) to oldest (front)
        let iter: Box<dyn Iterator<Item = &Arc<AdminEvent>>> = if let Some(seqs) = run_seqs {
            // Use the index: iterate seqs in reverse (they're sorted ascending)
            Box::new(seqs.iter().rev().filter_map(|s| self.by_seq.get(s)))
        } else {
            Box::new(self.events.iter().rev())
        };

        for event in iter {
            // Cursor: only events before cursor seq
            if let Some(c) = cursor {
                if event.seq >= c {
                    continue;
                }
            }

            // Time range filters
            if let Some(ref f) = from {
                if event.ts < *f {
                    continue;
                }
            }
            if let Some(ref t) = to {
                if event.ts > *t {
                    continue;
                }
            }

            // Text search: case-insensitive across payload, event_type, run_id, correlation_id
            if let Some(ref needle) = term_lower {
                let matches = event.payload.to_lowercase().contains(needle)
                    || event.event_type.to_lowercase().contains(needle)
                    || event.run_id.as_deref().map(|s| s.to_lowercase().contains(needle)).unwrap_or(false)
                    || event.correlation_id.as_deref().map(|s| s.to_lowercase().contains(needle)).unwrap_or(false);

                if !matches {
                    continue;
                }
            }

            results.push(Arc::clone(event));
            if results.len() >= limit {
                break;
            }
        }

        let next_cursor = if results.len() >= limit {
            results.last().map(|e| e.seq)
        } else {
            None
        };

        (results, next_cursor)
    }

    /// Get all events sharing the same correlation_id as the given event.
    /// Returns (events, root_seq) or None if not in cache.
    pub fn causal_tree(&self, seq: i64) -> Option<(Vec<Arc<AdminEvent>>, i64)> {
        let event = self.by_seq.get(&seq)?;
        let cid_str = event.correlation_id.as_deref()?;
        let cid = Uuid::parse_str(cid_str).ok()?;

        let seqs = self.by_correlation.get(&cid)?;
        let mut events: Vec<Arc<AdminEvent>> = seqs
            .iter()
            .filter_map(|s| self.by_seq.get(s).cloned())
            .collect();
        events.sort_by_key(|e| e.seq);

        // Root = event with no parent_id
        let root_seq = events
            .iter()
            .find(|e| e.parent_id.is_none())
            .map(|e| e.seq)
            .unwrap_or(seq);

        Some((events, root_seq))
    }

    /// Get all events for a run_id, ordered by seq ascending.
    /// Returns None if run_id not in cache.
    pub fn causal_flow(&self, run_id: &str) -> Option<Vec<Arc<AdminEvent>>> {
        let seqs = self.by_run.get(run_id)?;
        let mut events: Vec<Arc<AdminEvent>> = seqs
            .iter()
            .filter_map(|s| self.by_seq.get(s).cloned())
            .collect();
        events.sort_by_key(|e| e.seq);
        Some(events)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}

/// Thread-safe wrapper for the event cache.
pub type SharedEventCache = Arc<RwLock<EventCache>>;

/// Spawn a background task that listens to the EventBroadcast and feeds
/// new events into the cache.
pub fn spawn_cache_listener(
    cache: SharedEventCache,
    broadcast: &crate::event_broadcast::EventBroadcast,
) {
    let mut rx = broadcast.subscribe();

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let event = Arc::new(event);
                    cache.write().await.push(event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "Event cache listener lagged — some events may be missing until next restart");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    warn!("Event broadcast channel closed — cache listener stopping");
                    break;
                }
            }
        }
    });
}

// Re-use the row conversion from scout_run but keep it self-contained here
fn row_to_event_full(row: &sqlx::postgres::PgRow) -> EventRowFull {
    use sqlx::Row;
    EventRowFull {
        id: row.get("id"),
        parent_id: row.get("parent_id"),
        seq: row.get("seq"),
        ts: row.get("ts"),
        event_type: row.get("event_type"),
        data: row.get::<serde_json::Value, _>("data"),
        run_id: row.get("run_id"),
        correlation_id: row.get("correlation_id"),
        parent_seq: row.get("parent_seq"),
        handler_id: row.get("handler_id"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_event(seq: i64, event_type: &str, payload: &str, run_id: Option<&str>, correlation_id: Option<&str>, handler_id: Option<&str>, parent_id: Option<&str>) -> AdminEvent {
        AdminEvent {
            seq,
            ts: Utc::now(),
            event_type: event_type.to_string(),
            name: "test_event".to_string(),
            layer: "telemetry".to_string(),
            id: Some(Uuid::new_v4().to_string()),
            parent_id: parent_id.map(String::from),
            correlation_id: correlation_id.map(String::from),
            run_id: run_id.map(String::from),
            handler_id: handler_id.map(String::from),
            summary: None,
            payload: payload.to_string(),
        }
    }

    #[test]
    fn push_updates_all_indexes() {
        let mut cache = EventCache::new(10);
        let cid = Uuid::new_v4().to_string();
        let event = Arc::new(make_event(1, "TestEvent", r#"{"type":"test"}"#, Some("run-1"), Some(&cid), Some("handler-a"), None));

        cache.push(event);

        assert!(cache.by_seq.contains_key(&1));
        assert_eq!(cache.by_run.get("run-1").unwrap(), &vec![1i64]);
        assert_eq!(cache.by_handler.get("handler-a").unwrap(), &vec![1i64]);
        let cid_uuid = Uuid::parse_str(&cid).unwrap();
        assert_eq!(cache.by_correlation.get(&cid_uuid).unwrap(), &vec![1i64]);
    }

    #[test]
    fn cache_evicts_oldest_when_at_capacity() {
        let mut cache = EventCache::new(3);

        for i in 1..=4 {
            let event = Arc::new(make_event(i, "TestEvent", "{}", Some(&format!("run-{i}")), None, None, None));
            cache.push(event);
        }

        assert_eq!(cache.len(), 3);
        assert!(cache.by_seq.get(&1).is_none(), "oldest event should be evicted");
        assert!(cache.by_seq.get(&2).is_some());
        assert!(cache.by_seq.get(&4).is_some());
        assert!(cache.by_run.get("run-1").is_none(), "evicted event's run index should be cleaned");
    }

    #[test]
    fn eviction_removes_from_all_indexes() {
        let mut cache = EventCache::new(2);
        let cid = Uuid::new_v4().to_string();

        let e1 = Arc::new(make_event(1, "TestEvent", "{}", Some("run-x"), Some(&cid), Some("handler-y"), None));
        let e2 = Arc::new(make_event(2, "TestEvent", "{}", None, None, None, None));
        let e3 = Arc::new(make_event(3, "TestEvent", "{}", None, None, None, None));

        cache.push(e1);
        cache.push(e2);
        cache.push(e3); // evicts seq=1

        assert!(cache.by_seq.get(&1).is_none());
        assert!(cache.by_run.get("run-x").is_none());
        assert!(cache.by_handler.get("handler-y").is_none());
        let cid_uuid = Uuid::parse_str(&cid).unwrap();
        assert!(cache.by_correlation.get(&cid_uuid).is_none());
    }

    #[test]
    fn search_matches_payload_text_case_insensitive() {
        let mut cache = EventCache::new(100);
        cache.push(Arc::new(make_event(1, "WorldEvent", r#"{"type":"gathering_announced","title":"Community Meeting"}"#, None, None, None, None)));
        cache.push(Arc::new(make_event(2, "ScrapeEvent", r#"{"type":"url_scraped","url":"http://example.com"}"#, None, None, None, None)));

        let (results, _) = cache.search(Some("community"), None, None, None, None, 50);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].seq, 1);

        let (results, _) = cache.search(Some("EXAMPLE"), None, None, None, None, 50);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].seq, 2);
    }

    #[test]
    fn search_matches_event_type_and_run_id() {
        let mut cache = EventCache::new(100);
        cache.push(Arc::new(make_event(1, "WorldEvent", "{}", Some("abc-123"), None, None, None)));
        cache.push(Arc::new(make_event(2, "ScrapeEvent", "{}", Some("def-456"), None, None, None)));

        let (results, _) = cache.search(Some("worldevent"), None, None, None, None, 50);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].seq, 1);

        let (results, _) = cache.search(Some("abc-123"), None, None, None, None, 50);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].seq, 1);
    }

    #[test]
    fn causal_tree_returns_all_events_with_same_correlation_id() {
        let mut cache = EventCache::new(100);
        let cid = Uuid::new_v4().to_string();

        cache.push(Arc::new(make_event(1, "TestEvent", "{}", None, Some(&cid), None, None))); // root (no parent_id)
        cache.push(Arc::new(make_event(2, "TestEvent", "{}", None, Some(&cid), None, Some("parent-uuid"))));
        cache.push(Arc::new(make_event(3, "TestEvent", "{}", None, None, None, None))); // different correlation

        let (tree, root_seq) = cache.causal_tree(1).unwrap();
        assert_eq!(tree.len(), 2);
        assert_eq!(root_seq, 1);
    }

    #[test]
    fn causal_flow_returns_all_events_for_run_id() {
        let mut cache = EventCache::new(100);
        cache.push(Arc::new(make_event(1, "TestEvent", "{}", Some("run-a"), None, None, None)));
        cache.push(Arc::new(make_event(2, "TestEvent", "{}", Some("run-a"), None, None, None)));
        cache.push(Arc::new(make_event(3, "TestEvent", "{}", Some("run-b"), None, None, None)));

        let flow = cache.causal_flow("run-a").unwrap();
        assert_eq!(flow.len(), 2);
        assert_eq!(flow[0].seq, 1);
        assert_eq!(flow[1].seq, 2);

        assert!(cache.causal_flow("run-missing").is_none());
    }

    #[test]
    fn cursor_pagination_returns_correct_page() {
        let mut cache = EventCache::new(100);
        for i in 1..=10 {
            cache.push(Arc::new(make_event(i, "TestEvent", "{}", None, None, None, None)));
        }

        // First page: newest 3
        let (page1, cursor1) = cache.search(None, None, None, None, None, 3);
        assert_eq!(page1.len(), 3);
        assert_eq!(page1[0].seq, 10);
        assert_eq!(page1[2].seq, 8);
        assert_eq!(cursor1, Some(8));

        // Second page: next 3
        let (page2, cursor2) = cache.search(None, cursor1, None, None, None, 3);
        assert_eq!(page2.len(), 3);
        assert_eq!(page2[0].seq, 7);
        assert_eq!(page2[2].seq, 5);
        assert_eq!(cursor2, Some(5));

        // Last page
        let (page_last, cursor_last) = cache.search(None, Some(2), None, None, None, 3);
        assert_eq!(page_last.len(), 1);
        assert_eq!(page_last[0].seq, 1);
        assert_eq!(cursor_last, None);
    }
}
