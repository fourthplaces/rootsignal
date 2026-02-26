use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::{error, info};
use uuid::Uuid;

use rootsignal_common::{
    ActorNode, CitationNode, Node, NodeType, TagNode, TensionResponse,
    CONFIDENCE_DISPLAY_LIMITED,
};

use crate::reader::{
    extract_citation, fuzz_node, node_type_label, row_to_actor, row_to_node_by_label,
};
use crate::GraphClient;

/// In-memory snapshot of all displayable signals, actors, and relationships.
/// Signals are pre-fuzzed at load time. Expiry filtering is NOT pre-applied — it runs
/// at query time via `passes_display_filter()` since it depends on `Utc::now()`.
pub struct SignalCache {
    pub signals: Vec<Node>,
    pub actors: Vec<ActorNode>,

    pub signal_by_id: HashMap<Uuid, usize>,
    pub actor_by_id: HashMap<Uuid, usize>,

    pub citation_by_signal: HashMap<Uuid, Vec<CitationNode>>,
    pub actors_by_signal: HashMap<Uuid, Vec<usize>>,
    pub tension_responses: HashMap<Uuid, Vec<TensionResponse>>,

    pub tags: Vec<TagNode>,
    pub tag_by_id: HashMap<Uuid, usize>,
    pub tags_by_situation: HashMap<Uuid, Vec<usize>>,

    pub loaded_at: DateTime<Utc>,
}

impl SignalCache {
    pub async fn load(client: &GraphClient) -> Result<Self, neo4rs::Error> {
        let start = std::time::Instant::now();

        // Load signals and actors concurrently
        let (signals_result, actors_result) = tokio::join!(
            load_all_signals(client),
            load_all_actors(client),
        );

        let mut signals = signals_result?;
        let actors = actors_result?;

        // Apply coordinate fuzzing at load time
        for signal in &mut signals {
            *signal = fuzz_node(signal.clone());
        }

        // Build lookup indexes
        let signal_by_id: HashMap<Uuid, usize> = signals
            .iter()
            .enumerate()
            .filter_map(|(i, n)| n.meta().map(|m| (m.id, i)))
            .collect();

        let actor_by_id: HashMap<Uuid, usize> = actors
            .iter()
            .enumerate()
            .map(|(i, a)| (a.id, i))
            .collect();

        // Load tags
        let tags = load_all_tags(client).await?;
        let tag_by_id: HashMap<Uuid, usize> = tags
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i))
            .collect();

        // Load relationships concurrently
        let (citation_result, actor_signal_result, tension_resp_result, situation_tag_result) =
            tokio::join!(
                load_citations(client),
                load_actor_signal_edges(client),
                load_tension_responses(client),
                load_situation_tag_edges(client),
            );

        let citation_by_signal = citation_result?;

        // Build actors_by_signal map (signal_id -> vec of actor indices)
        let actor_signal_edges = actor_signal_result?;
        let mut actors_by_signal: HashMap<Uuid, Vec<usize>> = HashMap::new();
        for (signal_id, actor_id) in &actor_signal_edges {
            if let Some(&actor_idx) = actor_by_id.get(actor_id) {
                actors_by_signal
                    .entry(*signal_id)
                    .or_default()
                    .push(actor_idx);
            }
        }

        let tension_responses = tension_resp_result?;

        // Build tags_by_situation map (situation_id -> vec of tag indices)
        let situation_tag_edges = situation_tag_result?;
        let mut tags_by_situation: HashMap<Uuid, Vec<usize>> = HashMap::new();
        for (situation_id, tag_id) in &situation_tag_edges {
            if let Some(&tag_idx) = tag_by_id.get(tag_id) {
                let entries = tags_by_situation.entry(*situation_id).or_default();
                if !entries.contains(&tag_idx) {
                    entries.push(tag_idx);
                }
            }
        }

        let elapsed = start.elapsed();
        info!(
            signals = signals.len(),
            actors = actors.len(),
            tags = tags.len(),
            evidence_signals = citation_by_signal.len(),
            tension_responses = tension_responses.len(),
            elapsed_ms = elapsed.as_millis(),
            "Signal cache loaded"
        );

        Ok(Self {
            signals,
            actors,
            signal_by_id,
            actor_by_id,
            citation_by_signal,
            actors_by_signal,
            tension_responses,
            tags,
            tag_by_id,
            tags_by_situation,
            loaded_at: Utc::now(),
        })
    }
}

/// Thread-safe wrapper around `SignalCache` with atomic swap for lock-free reads.
pub struct CacheStore {
    inner: ArcSwap<SignalCache>,
    reloading: AtomicBool,
}

impl CacheStore {
    /// Create a new CacheStore with the given initial cache.
    pub fn new(initial: SignalCache) -> Self {
        Self {
            inner: ArcSwap::new(Arc::new(initial)),
            reloading: AtomicBool::new(false),
        }
    }

    /// Get a snapshot of the current cache. Returns an owned `Arc` so callers
    /// get a consistent view even if a reload swaps in new data.
    pub fn load_full(&self) -> Arc<SignalCache> {
        self.inner.load_full()
    }

    /// Reload the cache from Neo4j. Only one reload runs at a time.
    pub async fn reload(&self, client: &GraphClient) {
        if self
            .reloading
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            info!("Cache reload already in progress, skipping");
            return;
        }

        info!("Reloading signal cache from Neo4j");
        match SignalCache::load(client).await {
            Ok(new_cache) => {
                self.inner.store(Arc::new(new_cache));
                info!("Signal cache reloaded successfully");
            }
            Err(e) => {
                error!(error = %e, "Failed to reload signal cache, keeping stale data");
            }
        }

        self.reloading.store(false, Ordering::SeqCst);
    }

    /// Spawn a background loop that reloads the cache on a timer.
    pub fn spawn_reload_loop(self: &Arc<Self>, client: GraphClient) {
        let hours: u64 = std::env::var("CACHE_RELOAD_HOURS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);

        let store = Arc::clone(self);
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(hours * 3600);
            loop {
                tokio::time::sleep(interval).await;
                store.reload(&client).await;
            }
        });

        info!(interval_hours = hours, "Cache reload loop started");
    }
}

// --- Bulk load helpers ---

async fn load_all_signals(client: &GraphClient) -> Result<Vec<Node>, neo4rs::Error> {
    let all_types = [
        NodeType::Gathering,
        NodeType::Aid,
        NodeType::Need,
        NodeType::Notice,
        NodeType::Tension,
    ];

    let branches: Vec<String> = all_types
        .iter()
        .map(|nt| {
            let label = node_type_label(*nt);
            format!(
                "MATCH (n:{label})
                 WHERE n.confidence >= $min_confidence
                 RETURN n, labels(n)[0] AS node_label"
            )
        })
        .collect();

    let cypher = branches.join("\nUNION ALL\n");
    let q = query(&cypher).param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64);

    let mut signals = Vec::new();
    let mut stream = client.graph.execute(q).await?;
    while let Some(row) = stream.next().await? {
        if let Some(node) = row_to_node_by_label(&row) {
            signals.push(node);
        }
    }
    Ok(signals)
}

async fn load_all_actors(client: &GraphClient) -> Result<Vec<ActorNode>, neo4rs::Error> {
    let q = query("MATCH (a:Actor) RETURN a");
    let mut actors = Vec::new();
    let mut stream = client.graph.execute(q).await?;
    while let Some(row) = stream.next().await? {
        if let Some(actor) = row_to_actor(&row) {
            actors.push(actor);
        }
    }
    Ok(actors)
}

async fn load_citations(
    client: &GraphClient,
) -> Result<HashMap<Uuid, Vec<CitationNode>>, neo4rs::Error> {
    let cypher = "MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
         WHERE n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension
         RETURN n.id AS signal_id, collect(ev) AS evidence";

    let q = query(cypher);
    let mut map: HashMap<Uuid, Vec<CitationNode>> = HashMap::new();
    let mut stream = client.graph.execute(q).await?;

    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("signal_id").unwrap_or_default();
        if let Ok(id) = Uuid::parse_str(&id_str) {
            let evidence = extract_citation(&row);
            if !evidence.is_empty() {
                map.insert(id, evidence);
            }
        }
    }
    Ok(map)
}

/// Returns (signal_id, actor_id) pairs.
async fn load_actor_signal_edges(
    client: &GraphClient,
) -> Result<Vec<(Uuid, Uuid)>, neo4rs::Error> {
    let cypher = "MATCH (a:Actor)-[:ACTED_IN]->(n)
         WHERE n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension
         RETURN n.id AS signal_id, a.id AS actor_id";

    let q = query(cypher);
    let mut edges = Vec::new();
    let mut stream = client.graph.execute(q).await?;

    while let Some(row) = stream.next().await? {
        let sid_str: String = row.get("signal_id").unwrap_or_default();
        let aid_str: String = row.get("actor_id").unwrap_or_default();
        if let (Ok(sid), Ok(aid)) = (Uuid::parse_str(&sid_str), Uuid::parse_str(&aid_str)) {
            edges.push((sid, aid));
        }
    }
    Ok(edges)
}

async fn load_tension_responses(
    client: &GraphClient,
) -> Result<HashMap<Uuid, Vec<TensionResponse>>, neo4rs::Error> {
    let cypher = "MATCH (t:Tension)<-[rel:RESPONDS_TO|DRAWN_TO]-(n)
         WHERE n:Aid OR n:Gathering OR n:Need
         RETURN t.id AS tension_id, n, labels(n)[0] AS node_label,
                rel.match_strength AS match_strength, rel.explanation AS explanation";

    let q = query(cypher);
    let mut map: HashMap<Uuid, Vec<TensionResponse>> = HashMap::new();
    let mut stream = client.graph.execute(q).await?;

    while let Some(row) = stream.next().await? {
        let tid_str: String = row.get("tension_id").unwrap_or_default();
        let Ok(tid) = Uuid::parse_str(&tid_str) else {
            continue;
        };
        if let Some(node) = row_to_node_by_label(&row) {
            let match_strength: f64 = row.get("match_strength").unwrap_or(0.0);
            let explanation: String = row.get("explanation").unwrap_or_default();
            map.entry(tid).or_default().push(TensionResponse {
                node: fuzz_node(node),
                match_strength,
                explanation,
            });
        }
    }
    Ok(map)
}

async fn load_all_tags(client: &GraphClient) -> Result<Vec<TagNode>, neo4rs::Error> {
    let cypher = "MATCH (t:Tag) RETURN t.id AS id, t.slug AS slug, t.name AS name, t.created_at AS created_at";
    let q = query(cypher);
    let mut tags = Vec::new();
    let mut stream = client.graph.execute(q).await?;

    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("id").unwrap_or_default();
        let Ok(id) = Uuid::parse_str(&id_str) else {
            continue;
        };
        let slug: String = row.get("slug").unwrap_or_default();
        let name: String = row.get("name").unwrap_or_default();
        let created_at = crate::writer::row_datetime_opt_pub(&row, "created_at")
            .unwrap_or_else(Utc::now);

        tags.push(TagNode {
            id,
            slug,
            name,
            created_at,
        });
    }
    Ok(tags)
}

async fn load_situation_tag_edges(
    client: &GraphClient,
) -> Result<Vec<(Uuid, Uuid)>, neo4rs::Error> {
    let cypher = "MATCH (s:Situation)-[:TAGGED]->(t:Tag)
         RETURN s.id AS situation_id, t.id AS tag_id";
    let q = query(cypher);
    let mut edges = Vec::new();
    let mut stream = client.graph.execute(q).await?;

    while let Some(row) = stream.next().await? {
        let sid_str: String = row.get("situation_id").unwrap_or_default();
        let tid_str: String = row.get("tag_id").unwrap_or_default();
        if let (Ok(sid), Ok(tid)) = (Uuid::parse_str(&sid_str), Uuid::parse_str(&tid_str)) {
            edges.push((sid, tid));
        }
    }
    Ok(edges)
}

