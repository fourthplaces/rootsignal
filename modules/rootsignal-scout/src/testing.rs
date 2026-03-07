// Test mocks for the scout pipeline.
//
// Four mocks matching the four trait boundaries:
// - MockFetcher (ContentFetcher) — HashMap-based URL→response
// - MockSignalReader (SignalReader) — stateful in-memory graph
// - FixedEmbedder (TextEmbedder) — deterministic hash-based vectors
// - MockExtractor (SignalExtractor) — HashMap-based URL→ExtractionResult
//
// Plus test helpers for constructing ScoutScope, SourceNode, NodeMeta etc.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::canonical_value;
use rootsignal_common::safety::SensitivityLevel;
use rootsignal_common::types::{
    ActorNode, ArchivedFeed, ArchivedPage, ArchivedSearchResults, CitationNode, Node, NodeMeta,
    NodeType, Post, ReviewStatus, ScoutScope, SourceNode,
};
use rootsignal_graph::DuplicateMatch;

use crate::core::engine::{build_engine, ScoutEngine, ScoutEngineDeps};
use crate::core::extractor::{ExtractionResult, SignalExtractor};
use crate::infra::embedder::TextEmbedder;
use crate::traits::{ContentFetcher, SignalReader};

/// Standard embedding dimension for test vectors.
pub const TEST_EMBEDDING_DIM: usize = 64;

/// St. Paul, MN coordinates.
pub const ST_PAUL: (f64, f64) = (44.9537, -93.0900);
/// Duluth, MN coordinates.
pub const DULUTH: (f64, f64) = (46.7867, -92.1005);
/// New York, NY coordinates.
pub const NYC: (f64, f64) = (40.7128, -74.0060);
/// Dallas, TX coordinates.
pub const DALLAS: (f64, f64) = (32.7767, -96.7970);

/// HashMap-based content fetcher. Returns `Err` for unregistered URLs.
/// Builder pattern: `.on_page()`, `.on_search()`, `.on_posts()`, `.on_feed()`.
pub struct MockFetcher {
    pages: HashMap<String, ArchivedPage>,
    feeds: HashMap<String, ArchivedFeed>,
    posts: HashMap<String, Vec<Post>>,
    searches: HashMap<String, ArchivedSearchResults>,
    topic_searches: HashMap<String, Vec<Post>>,
    site_searches: HashMap<String, ArchivedSearchResults>,
}

impl MockFetcher {
    pub fn new() -> Self {
        Self {
            pages: HashMap::new(),
            feeds: HashMap::new(),
            posts: HashMap::new(),
            searches: HashMap::new(),
            topic_searches: HashMap::new(),
            site_searches: HashMap::new(),
        }
    }

    pub fn on_page(mut self, url: &str, page: ArchivedPage) -> Self {
        self.pages.insert(url.to_string(), page);
        self
    }

    #[allow(dead_code)] // scaffolding for future feed scrape tests
    pub fn on_feed(mut self, url: &str, feed: ArchivedFeed) -> Self {
        self.feeds.insert(url.to_string(), feed);
        self
    }

    pub fn on_posts(mut self, identifier: &str, posts: Vec<Post>) -> Self {
        self.posts.insert(identifier.to_string(), posts);
        self
    }

    pub fn on_search(mut self, query: &str, results: ArchivedSearchResults) -> Self {
        self.searches.insert(query.to_string(), results);
        self
    }

    pub fn on_topic_search(mut self, platform_url: &str, posts: Vec<Post>) -> Self {
        self.topic_searches.insert(platform_url.to_string(), posts);
        self
    }

    #[allow(dead_code)] // scaffolding for future site search tests
    pub fn on_site_search(mut self, query: &str, results: ArchivedSearchResults) -> Self {
        self.site_searches.insert(query.to_string(), results);
        self
    }
}

#[async_trait]
impl ContentFetcher for MockFetcher {
    async fn page(&self, url: &str) -> Result<ArchivedPage> {
        self.pages
            .get(url)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("MockFetcher: no page registered for {url}"))
    }

    async fn feed(&self, url: &str) -> Result<ArchivedFeed> {
        self.feeds
            .get(url)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("MockFetcher: no feed registered for {url}"))
    }

    async fn posts(&self, identifier: &str, _limit: u32) -> Result<Vec<Post>> {
        self.posts
            .get(identifier)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("MockFetcher: no posts registered for {identifier}"))
    }

    async fn search(&self, query: &str) -> Result<ArchivedSearchResults> {
        self.searches
            .get(query)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("MockFetcher: no search registered for {query}"))
    }

    async fn search_topics(
        &self,
        platform_url: &str,
        _topics: &[&str],
        _limit: u32,
    ) -> Result<Vec<Post>> {
        self.topic_searches
            .get(platform_url)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("MockFetcher: no topic search registered for {platform_url}")
            })
    }

    async fn site_search(&self, query: &str, _max_results: usize) -> Result<ArchivedSearchResults> {
        self.site_searches
            .get(query)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("MockFetcher: no site search registered for {query}"))
    }
}

// ---------------------------------------------------------------------------
// MockSignalReader
// ---------------------------------------------------------------------------

/// Stored signal entry in the mock graph.
#[derive(Debug, Clone)]
pub struct StoredSignal {
    pub id: Uuid,
    pub title: String,
    pub node_type: NodeType,
    pub source_url: String,
    pub corroboration_count: u32,
    pub embedding: Vec<f32>,
    pub about_location: Option<rootsignal_common::GeoPoint>,
    pub from_location: Option<rootsignal_common::GeoPoint>,
    pub published_at: Option<DateTime<Utc>>,
    pub about_location_name: Option<String>,
    pub confidence: f32,
    pub extracted_at: DateTime<Utc>,
}

/// Actor-signal link in the mock graph.
#[derive(Debug, Clone)]
pub struct ActorLink {
    pub actor_id: Uuid,
    pub signal_id: Uuid,
    pub role: String,
}

/// Inner mutable state for MockSignalReader.
struct MockSignalReaderInner {
    signals: HashMap<Uuid, StoredSignal>,
    /// (normalized_title, node_type) → signal_id for dedup lookups
    title_index: HashMap<(String, NodeType), Uuid>,
    /// source_url → vec of signal titles
    url_titles: HashMap<String, Vec<String>>,
    evidence: Vec<(Uuid, CitationNode)>,
    actors: HashMap<Uuid, ActorNode>,
    actor_by_name: HashMap<String, Uuid>,
    actor_links: Vec<ActorLink>,
    sources: HashMap<String, SourceNode>,
    resources: HashMap<String, Uuid>,
    resource_edges: Vec<(Uuid, String, String)>,
    tags: HashMap<Uuid, Vec<String>>,
    blocked: HashSet<String>,
    processed_hashes: HashSet<(String, String)>,
    fail_on_create: bool,
    /// (actor_id, source_id) — HAS_SOURCE edges
    actor_sources: Vec<(Uuid, Uuid)>,
    /// (signal_id, source_id) — PRODUCED_BY edges
    signal_sources: Vec<(Uuid, Uuid)>,
    /// canonical_key → actor_id for find_actor_by_canonical_key lookups
    actor_by_canonical_key: HashMap<String, Uuid>,
}

/// Stateful in-memory graph mock. Thread-safe via interior Mutex.
/// `create_node` inserts, `find_by_titles_and_types` queries, `corroborate` increments.
pub struct MockSignalReader {
    inner: Mutex<MockSignalReaderInner>,
}

impl MockSignalReader {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MockSignalReaderInner {
                signals: HashMap::new(),
                title_index: HashMap::new(),
                url_titles: HashMap::new(),
                evidence: Vec::new(),
                actors: HashMap::new(),
                actor_by_name: HashMap::new(),
                actor_links: Vec::new(),
                sources: HashMap::new(),
                resources: HashMap::new(),
                resource_edges: Vec::new(),
                tags: HashMap::new(),
                blocked: HashSet::new(),
                processed_hashes: HashSet::new(),
                fail_on_create: false,
                actor_sources: Vec::new(),
                signal_sources: Vec::new(),
                actor_by_canonical_key: HashMap::new(),
            }),
        }
    }

    /// Make `create_node` return an error for every call.
    pub fn failing_creates(self) -> Self {
        self.inner.lock().unwrap().fail_on_create = true;
        self
    }

    /// Pre-populate a blocked URL pattern.
    pub fn block_url(self, pattern: &str) -> Self {
        self.inner
            .lock()
            .unwrap()
            .blocked
            .insert(pattern.to_string());
        self
    }

    /// Pre-populate a processed content hash.
    pub fn with_processed_hash(self, hash: &str, url: &str) -> Self {
        self.inner
            .lock()
            .unwrap()
            .processed_hashes
            .insert((hash.to_string(), url.to_string()));
        self
    }

    // --- Assertion helpers ---

    pub fn signals_created(&self) -> usize {
        self.inner.lock().unwrap().signals.len()
    }

    pub fn has_signal_titled(&self, title: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        let normalized = title.trim().to_lowercase();
        inner
            .signals
            .values()
            .any(|s| s.title.trim().to_lowercase() == normalized)
    }

    pub fn signal_by_title(&self, title: &str) -> Option<StoredSignal> {
        let inner = self.inner.lock().unwrap();
        let normalized = title.trim().to_lowercase();
        inner
            .signals
            .values()
            .find(|s| s.title.trim().to_lowercase() == normalized)
            .cloned()
    }

    pub fn has_actor(&self, name: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.actor_by_name.contains_key(&name.to_lowercase())
    }

    pub fn actor_linked_to_signal(&self, actor_name: &str, signal_title: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        let actor_id = match inner.actor_by_name.get(&actor_name.to_lowercase()) {
            Some(id) => *id,
            None => return false,
        };
        let normalized_title = signal_title.trim().to_lowercase();
        let signal_id = match inner
            .signals
            .values()
            .find(|s| s.title.trim().to_lowercase() == normalized_title)
        {
            Some(s) => s.id,
            None => return false,
        };
        inner
            .actor_links
            .iter()
            .any(|l| l.actor_id == actor_id && l.signal_id == signal_id)
    }

    pub fn corroborations_for(&self, title: &str) -> u32 {
        let inner = self.inner.lock().unwrap();
        let normalized = title.trim().to_lowercase();
        inner
            .signals
            .values()
            .find(|s| s.title.trim().to_lowercase() == normalized)
            .map(|s| s.corroboration_count)
            .unwrap_or(0)
    }

    pub fn evidence_count_for(&self, signal_id: Uuid) -> usize {
        let inner = self.inner.lock().unwrap();
        inner
            .evidence
            .iter()
            .filter(|(id, _)| *id == signal_id)
            .count()
    }

    pub fn evidence_count_for_title(&self, title: &str) -> usize {
        let inner = self.inner.lock().unwrap();
        let normalized = title.trim().to_lowercase();
        let signal_id = inner
            .signals
            .values()
            .find(|s| s.title.trim().to_lowercase() == normalized)
            .map(|s| s.id);
        match signal_id {
            Some(id) => inner.evidence.iter().filter(|(eid, _)| *eid == id).count(),
            None => 0,
        }
    }

    pub fn evidence_count(&self) -> usize {
        self.inner.lock().unwrap().evidence.len()
    }

    pub fn actors_created(&self) -> usize {
        self.inner.lock().unwrap().actors.len()
    }

    // --- Setup helpers (for actor tests) ---

    /// Create or update an actor in the mock store (test setup only).
    pub async fn upsert_actor(&self, actor: &ActorNode) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .actor_by_name
            .insert(actor.name.to_lowercase(), actor.id);
        if !actor.canonical_key.is_empty() {
            inner
                .actor_by_canonical_key
                .insert(actor.canonical_key.clone(), actor.id);
        }
        inner.actors.insert(actor.id, actor.clone());
        Ok(())
    }

    /// Link an actor to a signal in the mock store (test setup only).
    pub async fn link_actor_to_signal(
        &self,
        actor_id: Uuid,
        signal_id: Uuid,
        role: &str,
    ) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.actor_links.push(ActorLink {
            actor_id,
            signal_id,
            role: role.to_string(),
        });
        Ok(())
    }

    /// Insert a signal node into the mock store (test setup only).
    pub async fn create_node(
        &self,
        node: &Node,
        embedding: &[f32],
        _created_by: &str,
        _run_id: &str,
    ) -> Result<Uuid> {
        let mut inner = self.inner.lock().unwrap();
        if inner.fail_on_create {
            bail!("MockSignalReader: create_node forced failure");
        }
        let id = node.id();
        let title = node.title().to_string();
        let node_type = node.node_type();
        let source_url = node
            .meta()
            .map(|m| m.source_url.clone())
            .unwrap_or_default();
        let normalized = title.trim().to_lowercase();

        let meta = node.meta();
        let stored = StoredSignal {
            id,
            title: title.clone(),
            node_type,
            source_url: source_url.clone(),
            corroboration_count: 0,
            embedding: embedding.to_vec(),
            about_location: meta.and_then(|m| m.about_location),
            from_location: meta.and_then(|m| m.from_location),
            published_at: meta.and_then(|m| m.published_at),
            about_location_name: meta.and_then(|m| m.about_location_name.clone()),
            confidence: meta.map(|m| m.confidence).unwrap_or(0.0),
            extracted_at: meta.map(|m| m.extracted_at).unwrap_or_else(Utc::now),
        };
        inner.signals.insert(id, stored);
        inner.title_index.insert((normalized, node_type), id);
        inner.url_titles.entry(source_url).or_default().push(title);
        Ok(id)
    }

    // --- Setup helpers (for dedup tests) ---

    /// Pre-populate URL→titles mapping for URL-based title dedup (Layer 2).
    pub fn add_url_titles(&self, url: &str, titles: Vec<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner
            .url_titles
            .entry(url.to_string())
            .or_default()
            .extend(titles);
    }

    /// Insert a signal directly into the mock store so `find_by_titles_and_types`
    /// will find it. Returns the generated signal ID.
    pub fn insert_signal(&self, title: &str, node_type: NodeType, source_url: &str) -> Uuid {
        let mut inner = self.inner.lock().unwrap();
        let id = Uuid::new_v4();
        let normalized = title.trim().to_lowercase();
        inner.signals.insert(
            id,
            StoredSignal {
                id,
                title: title.to_string(),
                node_type,
                source_url: source_url.to_string(),
                corroboration_count: 0,
                embedding: vec![],
                about_location: None,
                from_location: None,
                published_at: None,
                about_location_name: None,
                confidence: 0.5,
                extracted_at: Utc::now(),
            },
        );
        inner.title_index.insert((normalized, node_type), id);
        id
    }

    pub fn sources_promoted(&self) -> usize {
        self.inner.lock().unwrap().sources.len()
    }

    pub fn has_source_url(&self, url: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        let cv = canonical_value(url);
        inner.sources.contains_key(&cv)
    }

    pub fn has_resource_edge(&self, signal_title: &str, resource_slug: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        let normalized = signal_title.trim().to_lowercase();
        let signal_id = match inner
            .signals
            .values()
            .find(|s| s.title.trim().to_lowercase() == normalized)
        {
            Some(s) => s.id,
            None => return false,
        };
        inner
            .resource_edges
            .iter()
            .any(|(sid, slug, _)| *sid == signal_id && slug == resource_slug)
    }

    /// Check that a specific resource edge exists with an expected role (requires/prefers/offers).
    pub fn has_resource_edge_with_role(
        &self,
        signal_title: &str,
        resource_slug: &str,
        expected_role: &str,
    ) -> bool {
        let inner = self.inner.lock().unwrap();
        let normalized = signal_title.trim().to_lowercase();
        let signal_id = match inner
            .signals
            .values()
            .find(|s| s.title.trim().to_lowercase() == normalized)
        {
            Some(s) => s.id,
            None => return false,
        };
        inner.resource_edges.iter().any(|(sid, slug, role)| {
            *sid == signal_id && slug == resource_slug && role == expected_role
        })
    }

    /// Count resource edges for a signal.
    pub fn resource_edge_count_for(&self, signal_title: &str) -> usize {
        let inner = self.inner.lock().unwrap();
        let normalized = signal_title.trim().to_lowercase();
        let signal_id = match inner
            .signals
            .values()
            .find(|s| s.title.trim().to_lowercase() == normalized)
        {
            Some(s) => s.id,
            None => return 0,
        };
        inner
            .resource_edges
            .iter()
            .filter(|(sid, _, _)| *sid == signal_id)
            .count()
    }

    pub fn has_tag(&self, signal_title: &str, tag: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        let normalized = signal_title.trim().to_lowercase();
        let signal_id = match inner
            .signals
            .values()
            .find(|s| s.title.trim().to_lowercase() == normalized)
        {
            Some(s) => s.id,
            None => return false,
        };
        inner
            .tags
            .get(&signal_id)
            .map(|tags| tags.iter().any(|t| t == tag))
            .unwrap_or(false)
    }

    pub fn actor_count(&self) -> usize {
        self.inner.lock().unwrap().actors.len()
    }

    pub fn has_actor_with_canonical_key(&self, canonical_key: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.actor_by_canonical_key.contains_key(canonical_key)
    }

    pub fn actor_canonical_key(&self, actor_name: &str) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        let actor_id = inner.actor_by_name.get(&actor_name.to_lowercase())?;
        let actor = inner.actors.get(actor_id)?;
        Some(actor.canonical_key.clone())
    }

    pub fn actor_location_name(&self, actor_name: &str) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        let actor_id = inner.actor_by_name.get(&actor_name.to_lowercase())?;
        let actor = inner.actors.get(actor_id)?;
        actor.location_name.clone()
    }

    pub fn actor_location_coords(&self, actor_name: &str) -> Option<(f64, f64)> {
        let inner = self.inner.lock().unwrap();
        let actor_id = inner.actor_by_name.get(&actor_name.to_lowercase())?;
        let actor = inner.actors.get(actor_id)?;
        match (actor.location_lat, actor.location_lng) {
            (Some(lat), Some(lng)) => Some((lat, lng)),
            _ => None,
        }
    }

    pub fn actor_discovery_depth(&self, actor_name: &str) -> Option<u32> {
        let inner = self.inner.lock().unwrap();
        let actor_id = inner.actor_by_name.get(&actor_name.to_lowercase())?;
        let actor = inner.actors.get(actor_id)?;
        Some(actor.discovery_depth)
    }

    pub fn actor_has_source(&self, actor_name: &str, source_id: Uuid) -> bool {
        let inner = self.inner.lock().unwrap();
        let actor_id = match inner.actor_by_name.get(&actor_name.to_lowercase()) {
            Some(id) => *id,
            None => return false,
        };
        inner
            .actor_sources
            .iter()
            .any(|(aid, sid)| *aid == actor_id && *sid == source_id)
    }

    pub fn signal_has_source(&self, signal_title: &str, source_id: Uuid) -> bool {
        let inner = self.inner.lock().unwrap();
        let normalized = signal_title.trim().to_lowercase();
        let signal_id = match inner
            .signals
            .values()
            .find(|s| s.title.trim().to_lowercase() == normalized)
        {
            Some(s) => s.id,
            None => return false,
        };
        inner
            .signal_sources
            .iter()
            .any(|(sid, src)| *sid == signal_id && *src == source_id)
    }

    pub fn schedules_created(&self) -> usize {
        0
    }

    pub fn has_schedule_for(&self, _signal_title: &str) -> bool {
        false
    }

    pub fn schedule_for(&self, _signal_title: &str) -> Option<rootsignal_common::ScheduleNode> {
        None
    }
}

#[async_trait]
impl SignalReader for MockSignalReader {
    async fn blocked_urls(&self, urls: &[String]) -> Result<HashSet<String>> {
        let inner = self.inner.lock().unwrap();
        let blocked: HashSet<String> = urls
            .iter()
            .filter(|url| inner.blocked.iter().any(|p| url.contains(p)))
            .cloned()
            .collect();
        Ok(blocked)
    }

    async fn content_already_processed(&self, hash: &str, url: &str) -> Result<bool> {
        let inner = self.inner.lock().unwrap();
        Ok(inner
            .processed_hashes
            .contains(&(hash.to_string(), url.to_string())))
    }

    async fn signal_ids_for_url(&self, _url: &str) -> Result<Vec<(Uuid, NodeType)>> {
        Ok(Vec::new())
    }

    async fn read_corroboration_count(&self, id: Uuid, _node_type: NodeType) -> Result<u32> {
        let inner = self.inner.lock().unwrap();
        Ok(inner
            .signals
            .get(&id)
            .map(|s| s.corroboration_count)
            .unwrap_or(0))
    }

    async fn existing_titles_for_url(&self, url: &str) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.url_titles.get(url).cloned().unwrap_or_default())
    }

    async fn find_by_titles_and_types(
        &self,
        pairs: &[(String, NodeType)],
    ) -> Result<HashMap<(String, NodeType), (Uuid, String)>> {
        let inner = self.inner.lock().unwrap();
        let mut results = HashMap::new();
        for (title, nt) in pairs {
            let normalized = title.trim().to_lowercase();
            if let Some(id) = inner.title_index.get(&(normalized.clone(), *nt)) {
                if let Some(signal) = inner.signals.get(id) {
                    results.insert((normalized, *nt), (*id, signal.source_url.clone()));
                }
            }
        }
        Ok(results)
    }

    async fn find_duplicate(
        &self,
        _embedding: &[f32],
        _primary_type: NodeType,
        _threshold: f64,
        _min_lat: f64,
        _max_lat: f64,
        _min_lng: f64,
        _max_lng: f64,
    ) -> Result<Option<DuplicateMatch>> {
        // MockSignalReader doesn't do vector similarity by default.
        // Chain tests that need dedup behavior should pre-populate via create_node
        // and rely on title-based dedup (find_by_titles_and_types).
        Ok(None)
    }

    async fn find_actor_by_name(&self, name: &str) -> Result<Option<Uuid>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.actor_by_name.get(&name.to_lowercase()).copied())
    }

    async fn find_actor_by_canonical_key(&self, canonical_key: &str) -> Result<Option<Uuid>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.actor_by_canonical_key.get(canonical_key).copied())
    }

    async fn get_active_sources(&self) -> Result<Vec<SourceNode>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.sources.values().cloned().collect())
    }

    async fn find_expired_signals(&self) -> Result<Vec<(Uuid, NodeType, String)>> {
        Ok(vec![])
    }

    async fn get_signals_for_actor(
        &self,
        actor_id: Uuid,
    ) -> Result<Vec<(f64, f64, String, DateTime<Utc>)>> {
        let inner = self.inner.lock().unwrap();
        let mut results = Vec::new();
        for link in &inner.actor_links {
            if link.actor_id == actor_id && link.role == "authored" {
                if let Some(signal) = inner.signals.get(&link.signal_id) {
                    if let Some(ref loc) = signal.about_location {
                        let name = signal.about_location_name.clone().unwrap_or_default();
                        results.push((loc.lat, loc.lng, name, signal.extracted_at));
                    }
                }
            }
        }
        Ok(results)
    }

    async fn list_all_actors(&self) -> Result<Vec<(ActorNode, Vec<SourceNode>)>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner
            .actors
            .values()
            .map(|a| (a.clone(), Vec::new()))
            .collect())
    }
}

// ---------------------------------------------------------------------------
// FixedEmbedder
// ---------------------------------------------------------------------------

/// Deterministic embedder for testing. Registered texts get exact vectors;
/// unmatched texts get a unique hash-based vector (low similarity to everything).
pub struct FixedEmbedder {
    vectors: HashMap<String, Vec<f32>>,
    dimension: usize,
}

impl FixedEmbedder {
    pub fn new(dimension: usize) -> Self {
        Self {
            vectors: HashMap::new(),
            dimension,
        }
    }

    /// Register a text→vector mapping for controlled similarity.
    pub fn on_text(mut self, text: &str, vector: Vec<f32>) -> Self {
        self.vectors.insert(text.to_string(), vector);
        self
    }

    /// Generate a deterministic hash-based vector for unmatched text.
    fn hash_vector(&self, text: &str) -> Vec<f32> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        let seed = hasher.finish();

        let mut vec = vec![0.0f32; self.dimension];
        let mut state = seed;
        for v in vec.iter_mut() {
            // Simple LCG PRNG
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *v = ((state >> 33) as f32 / u32::MAX as f32) * 2.0 - 1.0;
        }
        // Normalize to unit vector
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in vec.iter_mut() {
                *v /= norm;
            }
        }
        vec
    }
}

#[async_trait]
impl rootsignal_common::TextEmbedder for FixedEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self
            .vectors
            .get(text)
            .cloned()
            .unwrap_or_else(|| self.hash_vector(text)))
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                self.vectors
                    .get(t.as_str())
                    .cloned()
                    .unwrap_or_else(|| self.hash_vector(t))
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// MockExtractor
// ---------------------------------------------------------------------------

/// HashMap-based signal extractor. Returns `Err` for unregistered URLs.
pub struct MockExtractor {
    results: HashMap<String, ExtractionResult>,
    /// Fallback result for any unregistered URL (optional).
    default_result: Option<ExtractionResult>,
}

impl MockExtractor {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
            default_result: None,
        }
    }

    /// Register a URL→ExtractionResult mapping.
    pub fn on_url(mut self, url: &str, result: ExtractionResult) -> Self {
        self.results.insert(url.to_string(), result);
        self
    }

    /// Set a default result for any URL not explicitly registered.
    pub fn with_default(mut self, result: ExtractionResult) -> Self {
        self.default_result = Some(result);
        self
    }
}

#[async_trait]
impl SignalExtractor for MockExtractor {
    async fn extract(&self, _content: &str, source_url: &str) -> Result<ExtractionResult> {
        if let Some(result) = self.results.get(source_url) {
            return Ok(ExtractionResult {
                nodes: result.nodes.clone(),
                implied_queries: result.implied_queries.clone(),
                resource_tags: result.resource_tags.clone(),
                signal_tags: result.signal_tags.clone(),
                raw_signal_count: result.raw_signal_count,
                rejected: result.rejected.clone(),
                schedules: result.schedules.clone(),
                author_actors: result.author_actors.clone(),
                categories: result.categories.clone(),
                logs: vec![],
            });
        }
        if let Some(ref default) = self.default_result {
            return Ok(ExtractionResult {
                nodes: default.nodes.clone(),
                implied_queries: default.implied_queries.clone(),
                resource_tags: default.resource_tags.clone(),
                signal_tags: default.signal_tags.clone(),
                raw_signal_count: default.raw_signal_count,
                rejected: default.rejected.clone(),
                schedules: default.schedules.clone(),
                author_actors: default.author_actors.clone(),
                categories: default.categories.clone(),
                logs: vec![],
            });
        }
        bail!("MockExtractor: no result registered for {source_url}")
    }
}

// ---------------------------------------------------------------------------
// MockAgent — canned JSON for ai_extract calls
// ---------------------------------------------------------------------------

/// A mock AI agent that returns pre-configured JSON from `extract_json` calls.
/// Implements the `Agent` trait from `ai_client`.
pub struct MockAgent {
    extract_response: Mutex<Option<serde_json::Value>>,
    should_error: bool,
}

impl MockAgent {
    /// Create a MockAgent that returns the given JSON from `extract_json`.
    pub fn with_response(response: serde_json::Value) -> Self {
        Self {
            extract_response: Mutex::new(Some(response)),
            should_error: false,
        }
    }

    /// Create a MockAgent that always returns an error.
    pub fn failing() -> Self {
        Self {
            extract_response: Mutex::new(None),
            should_error: true,
        }
    }
}

#[async_trait]
impl ai_client::Agent for MockAgent {
    async fn extract_json(
        &self,
        _system: &str,
        _user: &str,
        _schema: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if self.should_error {
            bail!("MockAgent: configured to fail");
        }
        self.extract_response
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow::anyhow!("MockAgent: no response configured"))
    }

    async fn chat(&self, _system: &str, _user: &str) -> Result<String> {
        bail!("MockAgent: chat not implemented")
    }

    fn with_tools(
        &self,
        _tools: Vec<Arc<dyn ai_client::tool::DynTool>>,
    ) -> Box<dyn ai_client::Agent> {
        unimplemented!("MockAgent: with_tools not needed in tests")
    }

    fn prompt(&self, _input: &str) -> Box<dyn ai_client::PromptBuilder> {
        unimplemented!("MockAgent: prompt not needed in tests")
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Create a Tension node with just a title (no location).
pub fn tension(title: &str) -> Node {
    use rootsignal_common::types::{NodeMeta, Severity, ConcernNode};
    Node::Concern(ConcernNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: None,
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        severity: Severity::Medium,
        subject: None,
        opposing: None,
    })
}

/// Create a Tension node with a title and geographic coordinates.
pub fn tension_at(title: &str, lat: f64, lng: f64) -> Node {
    use rootsignal_common::types::{GeoPoint, NodeMeta, Severity, ConcernNode};
    use rootsignal_common::GeoPrecision;
    Node::Concern(ConcernNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: Some(GeoPoint {
                lat,
                lng,
                precision: GeoPrecision::Approximate,
            }),
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        severity: Severity::Medium,
        subject: None,
        opposing: None,
    })
}

/// Create a Need node with just a title (no location).
pub fn need(title: &str) -> Node {
    use rootsignal_common::types::{HelpRequestNode, NodeMeta, Urgency};
    Node::HelpRequest(HelpRequestNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: None,
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        urgency: Urgency::Medium,
        what_needed: None,
        action_url: None,
        stated_goal: None,
    })
}

/// Create a Need node with a title and geographic coordinates.
pub fn need_at(title: &str, lat: f64, lng: f64) -> Node {
    use rootsignal_common::types::{GeoPoint, HelpRequestNode, NodeMeta, Urgency};
    use rootsignal_common::GeoPrecision;
    Node::HelpRequest(HelpRequestNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: Some(GeoPoint {
                lat,
                lng,
                precision: GeoPrecision::Approximate,
            }),
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        urgency: Urgency::Medium,
        what_needed: None,
        action_url: None,
        stated_goal: None,
    })
}

/// Create a Gathering node with just a title (no location).
pub fn gathering(title: &str) -> Node {
    use rootsignal_common::types::{GatheringNode, NodeMeta};
    Node::Gathering(GatheringNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: None,
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        starts_at: None,
        ends_at: None,
        action_url: String::new(),
        organizer: None,
        is_recurring: false,
    })
}

/// Create a Gathering node with a title and geographic coordinates.
pub fn gathering_at(title: &str, lat: f64, lng: f64) -> Node {
    use rootsignal_common::types::{GatheringNode, GeoPoint, NodeMeta};
    use rootsignal_common::GeoPrecision;
    Node::Gathering(GatheringNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: Some(GeoPoint {
                lat,
                lng,
                precision: GeoPrecision::Approximate,
            }),
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        starts_at: None,
        ends_at: None,
        action_url: String::new(),
        organizer: None,
        is_recurring: false,
    })
}

/// Create an Aid node with just a title (no location).
pub fn aid(title: &str) -> Node {
    use rootsignal_common::types::{ResourceOfferNode, NodeMeta};
    Node::Resource(ResourceOfferNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: None,
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        action_url: String::new(),
        availability: None,
        eligibility: None,
        is_ongoing: false,
    })
}

/// Create an Aid node with a title and geographic coordinates.
pub fn aid_at(title: &str, lat: f64, lng: f64) -> Node {
    use rootsignal_common::types::{ResourceOfferNode, GeoPoint, NodeMeta};
    use rootsignal_common::GeoPrecision;
    Node::Resource(ResourceOfferNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: Some(GeoPoint {
                lat,
                lng,
                precision: GeoPrecision::Approximate,
            }),
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        action_url: String::new(),
        availability: None,
        eligibility: None,
        is_ongoing: false,
    })
}

/// Create a Notice node with just a title (no location).
pub fn notice(title: &str) -> Node {
    use rootsignal_common::types::{NodeMeta, AnnouncementNode, Severity};
    Node::Announcement(AnnouncementNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: None,
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        severity: Severity::Medium,
        subject: None,
        effective_date: None,
        source_authority: None,
    })
}

/// Create a Notice node with a title and geographic coordinates.
pub fn notice_at(title: &str, lat: f64, lng: f64) -> Node {
    use rootsignal_common::types::{GeoPoint, NodeMeta, AnnouncementNode, Severity};
    use rootsignal_common::GeoPrecision;
    Node::Announcement(AnnouncementNode {
        meta: NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: String::new(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,

            corroboration_count: 0,
            about_location: Some(GeoPoint {
                lat,
                lng,
                precision: GeoPrecision::Approximate,
            }),
            about_location_name: None,
            from_location: None,
            source_url: String::new(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,

            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
            category: None,
        },
        severity: Severity::Medium,
        subject: None,
        effective_date: None,
        source_authority: None,
    })
}

/// Create a Minneapolis-area ScoutScope for testing.
pub fn mpls_region() -> ScoutScope {
    ScoutScope {
        center_lat: 44.9778,
        center_lng: -93.2650,
        radius_km: 15.0,
        name: "Minneapolis".to_string(),
    }
}

/// Create a web query SourceNode.
pub fn web_query_source(query: &str) -> SourceNode {
    SourceNode::new(
        canonical_value(query),
        canonical_value(query),
        None,
        rootsignal_common::DiscoveryMethod::Curated,
        1.0,
        rootsignal_common::SourceRole::Mixed,
        None,
    )
}

/// Create a web page SourceNode.
pub fn page_source(url: &str) -> SourceNode {
    SourceNode::new(
        canonical_value(url),
        canonical_value(url),
        Some(url.to_string()),
        rootsignal_common::DiscoveryMethod::Curated,
        1.0,
        rootsignal_common::SourceRole::Mixed,
        None,
    )
}

/// Create a social media SourceNode.
pub fn social_source(url: &str) -> SourceNode {
    SourceNode::new(
        canonical_value(url),
        canonical_value(url),
        Some(url.to_string()),
        rootsignal_common::DiscoveryMethod::Curated,
        1.0,
        rootsignal_common::SourceRole::Mixed,
        None,
    )
}

/// Build a minimal SourcesPrepared for tests.
///
/// `include_social`: if true, includes a social source in tension phase.
pub fn sources_prepared_event(include_social: bool) -> crate::domains::lifecycle::events::LifecycleEvent {
    use crate::core::aggregate::SourcePlan;

    let web = page_source("https://example.com/page");
    let social = social_source("https://instagram.com/test_account");

    let mut selected = vec![web.clone()];
    let mut tension_keys: HashSet<String> = HashSet::from([web.canonical_key.clone()]);

    if include_social {
        selected.push(social.clone());
        tension_keys.insert(social.canonical_key.clone());
    }

    let plan = SourcePlan {
        all_sources: selected.clone(),
        selected_sources: selected,
        tension_phase_keys: tension_keys,
        response_phase_keys: HashSet::new(),
        selected_keys: HashSet::new(),
        consumed_pin_ids: Vec::new(),
    };

    crate::domains::lifecycle::events::LifecycleEvent::SourcesPrepared {
        tension_count: plan.tension_phase_keys.len() as u32,
        response_count: 0,
        source_plan: plan,
        actor_contexts: HashMap::new(),
        url_mappings: HashMap::new(),
        web_urls: Vec::new(),
        web_source_keys: HashMap::new(),
        web_source_count: 0,
        pub_dates: HashMap::new(),
        query_api_errors: HashSet::new(),
    }
}

/// Create a minimal Post for testing social scrape.
pub fn test_post(text: &str) -> Post {
    Post {
        id: Uuid::new_v4(),
        source_id: Uuid::new_v4(),
        fetched_at: Utc::now(),
        content_hash: String::new(),
        text: Some(text.to_string()),
        author: None,
        location: None,
        engagement: None,
        published_at: None,
        permalink: None,
        mentions: Vec::new(),
        hashtags: Vec::new(),
        media_type: None,
        platform_id: None,
        attachments: Vec::new(),
    }
}

/// Create a default NodeMeta for testing.
pub fn test_meta(source_url: &str) -> NodeMeta {
    NodeMeta {
        id: Uuid::new_v4(),
        title: String::new(),
        summary: String::new(),
        sensitivity: SensitivityLevel::General,
        confidence: 0.8,
        corroboration_count: 0,
        about_location: None,
        about_location_name: None,
        from_location: None,
        source_url: source_url.to_string(),
        extracted_at: Utc::now(),
        published_at: None,
        last_confirmed_active: Utc::now(),
        source_diversity: 1,
        cause_heat: 0.0,
        implied_queries: Vec::new(),
        channel_diversity: 1,
        review_status: ReviewStatus::Staged,
        was_corrected: false,
        corrections: None,
        rejection_reason: None,
        mentioned_actors: Vec::new(),
        category: None,
    }
}

/// Create a minimal ArchivedPage for testing.
pub fn archived_page(url: &str, markdown: &str) -> ArchivedPage {
    ArchivedPage {
        id: Uuid::new_v4(),
        source_id: Uuid::new_v4(),
        fetched_at: Utc::now(),
        content_hash: {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            markdown.hash(&mut h);
            format!("{:016x}", h.finish())
        },
        raw_html: String::new(),
        markdown: markdown.to_string(),
        title: None,
        links: Vec::new(),
        published_at: None,
    }
}

/// Create a test engine with a dummy store, no event store, no projector.
pub fn test_engine() -> Arc<ScoutEngine> {
    test_engine_for_store(
        Arc::new(MockSignalReader::new()) as Arc<dyn SignalReader>,
    )
}

/// Create a test engine wired to the given store.
pub fn test_engine_for_store(
    store: Arc<dyn SignalReader>,
) -> Arc<ScoutEngine> {
    test_engine_for_store_with_embedder(
        store,
        Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
    )
}

/// Create a test engine wired to the given store and embedder.
pub fn test_engine_for_store_with_embedder(
    store: Arc<dyn SignalReader>,
    embedder: Arc<dyn TextEmbedder>,
) -> Arc<ScoutEngine> {
    Arc::new(build_engine(
        ScoutEngineDeps::new(store, embedder, Uuid::new_v4()),
        None,
    ))
}

/// Create a test engine that captures all dispatched events for inspection.
pub fn test_engine_with_capture() -> (
    Arc<ScoutEngine>,
    Arc<Mutex<Vec<seesaw_core::AnyEvent>>>,
) {
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        Arc::new(MockSignalReader::new()) as Arc<dyn SignalReader>,
        None,
    );
    (engine, captured)
}

/// Create a test engine with capture, wired to the given store and optional region.
///
/// Returns `(engine, captured, scope)` — caller emits `ScoutRunRequested { run_id, scope }`
/// to populate PipelineState.run_scope, matching production flow.
pub fn test_engine_with_capture_for_store(
    store: Arc<dyn SignalReader>,
    region: Option<rootsignal_common::ScoutScope>,
) -> (
    Arc<ScoutEngine>,
    Arc<Mutex<Vec<seesaw_core::AnyEvent>>>,
    crate::core::run_scope::RunScope,
) {
    let scope = match region {
        Some(r) => crate::core::run_scope::RunScope::Region(r),
        None => crate::core::run_scope::RunScope::Unscoped,
    };
    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut deps = ScoutEngineDeps::new(
        store,
        Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        Uuid::new_v4(),
    );
    deps.captured_events = Some(captured.clone());
    let engine = Arc::new(build_engine(deps, None));
    (engine, captured, scope)
}

/// Create a test engine with capture, AI agent, and optional region.
///
/// Returns `(engine, captured, scope)` — caller emits `ScoutRunRequested { run_id, scope }`
/// to populate PipelineState.run_scope, matching production flow.
pub fn test_engine_with_ai(
    store: Arc<dyn SignalReader>,
    ai: Arc<dyn ai_client::Agent>,
    region: Option<rootsignal_common::ScoutScope>,
) -> (
    Arc<ScoutEngine>,
    Arc<Mutex<Vec<seesaw_core::AnyEvent>>>,
    crate::core::run_scope::RunScope,
) {
    let scope = match region {
        Some(r) => crate::core::run_scope::RunScope::Region(r),
        None => crate::core::run_scope::RunScope::Unscoped,
    };
    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut deps = ScoutEngineDeps::new(
        store,
        Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        Uuid::new_v4(),
    );
    deps.ai = Some(ai);
    deps.captured_events = Some(captured.clone());
    let engine = Arc::new(build_engine(deps, None));
    (engine, captured, scope)
}

/// Create a test engine for a source-targeted run with real extractor, event capture,
/// and all scrape deps wired. Uses the real `Extractor` backed by the given AI agent.
///
/// Returns `(engine, captured, scope)` — caller emits `ScoutRunRequested { run_id, scope }`
/// to populate PipelineState.run_scope, matching production flow.
pub fn test_engine_for_source_run(
    store: Arc<dyn SignalReader>,
    sources: Vec<rootsignal_common::SourceNode>,
    fetcher: Arc<dyn ContentFetcher>,
    ai: Arc<dyn ai_client::Agent>,
) -> (
    Arc<ScoutEngine>,
    Arc<Mutex<Vec<seesaw_core::AnyEvent>>>,
    crate::core::run_scope::RunScope,
) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let region = mpls_region();
    let extractor: Arc<dyn crate::core::extractor::SignalExtractor> = Arc::new(
        crate::core::extractor::Extractor::new(
            ai.clone(),
            &region.name,
            region.center_lat,
            region.center_lng,
        ),
    );
    let scope = crate::core::run_scope::RunScope::Sources {
        sources,
        region: Some(region),
    };
    let mut deps = ScoutEngineDeps::new(
        store,
        Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        Uuid::new_v4(),
    );
    deps.fetcher = Some(fetcher);
    deps.extractor = Some(extractor);
    deps.ai = Some(ai);
    deps.captured_events = Some(captured.clone());
    let engine = Arc::new(build_engine(deps, None));
    (engine, captured, scope)
}

/// Create a test ScoutEngineDeps with a given store (for activity-level tests).
pub fn test_scout_deps(
    store: Arc<dyn SignalReader>,
) -> ScoutEngineDeps {
    ScoutEngineDeps::new(
        store,
        Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        Uuid::new_v4(),
    )
}

/// Create a test ScoutEngineDeps pre-loaded with scrape deps (store + extractor + fetcher).
pub fn test_scrape_deps(
    store: Arc<dyn SignalReader>,
    extractor: Arc<dyn crate::core::extractor::SignalExtractor>,
    fetcher: Arc<dyn ContentFetcher>,
) -> ScoutEngineDeps {
    let mut deps = test_scout_deps(store);
    deps.extractor = Some(extractor);
    deps.fetcher = Some(fetcher);
    deps
}

/// Create a minimal ArchivedSearchResults for testing.
pub fn search_results(query: &str, urls: &[&str]) -> ArchivedSearchResults {
    ArchivedSearchResults {
        id: Uuid::new_v4(),
        source_id: Uuid::new_v4(),
        fetched_at: Utc::now(),
        content_hash: String::new(),
        query: query.to_string(),
        results: urls
            .iter()
            .map(|url| rootsignal_common::SearchResult {
                url: url.to_string(),
                title: String::new(),
                snippet: String::new(),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Scrape completion test builders
// ---------------------------------------------------------------------------

use typed_builder::TypedBuilder;

use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::domains::scrape::activities::UrlExtraction;
use crate::domains::scrape::events::ScrapeEvent;

#[derive(TypedBuilder)]
pub struct TestWebScrapeCompleted {
    #[builder(default = true)]
    is_tension: bool,
    #[builder(default)]
    urls_scraped: u32,
    #[builder(default)]
    signals_extracted: u32,
    #[builder(default)]
    source_signal_counts: HashMap<String, u32>,
    #[builder(default)]
    collected_links: Vec<CollectedLink>,
    #[builder(default)]
    extracted_batches: Vec<UrlExtraction>,
    #[builder(default)]
    page_previews: HashMap<String, String>,
    #[builder(default)]
    expansion_queries: Vec<String>,
}

impl From<TestWebScrapeCompleted> for ScrapeEvent {
    fn from(t: TestWebScrapeCompleted) -> Self {
        ScrapeEvent::WebScrapeCompleted {
            run_id: Uuid::new_v4(),
            is_tension: t.is_tension,
            urls_scraped: t.urls_scraped,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: t.signals_extracted,
            source_signal_counts: t.source_signal_counts,
            collected_links: t.collected_links,
            expansion_queries: t.expansion_queries,
            page_previews: t.page_previews,
            extracted_batches: t.extracted_batches,
        }
    }
}

/// Build an empty SocialScrapeCompleted for tests.
pub fn empty_social_scrape(is_tension: bool) -> ScrapeEvent {
    ScrapeEvent::SocialScrapeCompleted {
        run_id: Uuid::new_v4(),
        is_tension,
        sources_scraped: 0,
        signals_extracted: 0,
        source_signal_counts: Default::default(),
        collected_links: Default::default(),
        expansion_queries: Default::default(),
        stats_delta: Default::default(),
        extracted_batches: Default::default(),
    }
}

/// Build an empty TopicDiscoveryCompleted for completing role sets in tests.
pub fn empty_topic_discovery() -> ScrapeEvent {
    ScrapeEvent::TopicDiscoveryCompleted {
        run_id: Uuid::new_v4(),
        source_signal_counts: Default::default(),
        collected_links: Default::default(),
        expansion_queries: Default::default(),
        stats_delta: Default::default(),
        extracted_batches: Default::default(),
    }
}

// ---------------------------------------------------------------------------
// MockSignalReader self-tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tension_with_url(title: &str, source_url: &str) -> Node {
        let mut node = tension(title);
        if let Some(meta) = node.meta_mut() {
            meta.source_url = source_url.to_string();
        }
        node
    }

    #[tokio::test]
    async fn create_then_find_returns_created_signal() {
        let store = MockSignalReader::new();
        let node = tension_with_url("Housing Crisis Downtown", "https://example.com");
        let id = store
            .create_node(&node, &[0.1, 0.2, 0.3], "test", "run-1")
            .await
            .unwrap();

        assert!(store.has_signal_titled("Housing Crisis Downtown"));
        assert!(store.has_signal_titled("housing crisis downtown")); // case-insensitive

        let results = store
            .find_by_titles_and_types(&[("housing crisis downtown".to_string(), NodeType::Concern)])
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        let (found_id, found_url) = results
            .get(&("housing crisis downtown".to_string(), NodeType::Concern))
            .unwrap();
        assert_eq!(*found_id, id);
        assert_eq!(found_url, "https://example.com");
    }

    #[tokio::test]
    async fn find_by_titles_returns_empty_for_unknown() {
        let store = MockSignalReader::new();
        let results = store
            .find_by_titles_and_types(&[("nonexistent signal".to_string(), NodeType::Concern)])
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn actor_lifecycle() {
        let store = MockSignalReader::new();
        let node = tension_with_url("Free Legal Clinic", "https://example.com");
        let signal_id = store
            .create_node(&node, &[0.1, 0.2], "test", "run-1")
            .await
            .unwrap();

        // Actor doesn't exist yet
        assert!(!store.has_actor("Legal Aid Org"));

        // Create actor
        let actor = ActorNode {
            id: Uuid::new_v4(),
            name: "Legal Aid Org".to_string(),
            actor_type: rootsignal_common::ActorType::Organization,
            canonical_key: "legal-aid-org".to_string(),
            domains: vec![],
            social_urls: vec![],
            description: String::new(),
            signal_count: 0,
            first_seen: Utc::now(),
            last_active: Utc::now(),
            typical_roles: vec![],
            bio: None,
            location_lat: None,
            location_lng: None,
            location_name: None,
            discovery_depth: 0,
        };
        store.upsert_actor(&actor).await.unwrap();
        assert!(store.has_actor("Legal Aid Org"));

        // Link actor to signal
        store
            .link_actor_to_signal(actor.id, signal_id, "mentioned")
            .await
            .unwrap();
        assert!(store.actor_linked_to_signal("Legal Aid Org", "Free Legal Clinic"));
    }
}
