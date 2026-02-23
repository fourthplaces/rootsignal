// Trait abstractions for ScrapePhase dependencies.
//
// ContentFetcher replaces Arc<Archive> — all content fetching behind one trait.
// SignalStore replaces GraphWriter — all graph writes behind one trait.
//
// These enable deterministic testing with MockFetcher and MockSignalStore:
// no network, no database, no Docker. `cargo test` in seconds.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::types::{
    ActorNode, ArchivedFeed, ArchivedPage, ArchivedSearchResults, EvidenceNode, Node, NodeType,
    Post, SourceNode,
};
use rootsignal_common::EntityMappingOwned;
use rootsignal_graph::DuplicateMatch;

// ---------------------------------------------------------------------------
// ContentFetcher — replaces Arc<Archive>
// ---------------------------------------------------------------------------

#[async_trait]
pub trait ContentFetcher: Send + Sync {
    /// Fetch and render a web page to markdown.
    async fn page(&self, url: &str) -> Result<ArchivedPage>;

    /// Fetch an RSS/Atom feed.
    async fn feed(&self, url: &str) -> Result<ArchivedFeed>;

    /// Fetch social media posts for an account.
    async fn posts(&self, identifier: &str, limit: u32) -> Result<Vec<Post>>;

    /// Run a web search query (Serper).
    async fn search(&self, query: &str) -> Result<ArchivedSearchResults>;

    /// Search social platforms by topic keywords. Absorbs the
    /// `archive.source(platform_url).search_topics(topics, limit)` two-step.
    async fn search_topics(
        &self,
        platform_url: &str,
        topics: &[&str],
        limit: u32,
    ) -> Result<Vec<Post>>;

    /// Site-scoped web search. Absorbs the
    /// `archive.source(query).search(query).max_results(n)` two-step.
    async fn site_search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<ArchivedSearchResults>;
}

#[async_trait]
impl ContentFetcher for rootsignal_archive::Archive {
    async fn page(&self, url: &str) -> Result<ArchivedPage> {
        Ok(self.page(url).await?)
    }

    async fn feed(&self, url: &str) -> Result<ArchivedFeed> {
        Ok(self.feed(url).await?)
    }

    async fn posts(&self, identifier: &str, limit: u32) -> Result<Vec<Post>> {
        Ok(self.posts(identifier, limit).await?)
    }

    async fn search(&self, query: &str) -> Result<ArchivedSearchResults> {
        Ok(self.search(query).await?)
    }

    async fn search_topics(
        &self,
        platform_url: &str,
        topics: &[&str],
        limit: u32,
    ) -> Result<Vec<Post>> {
        let handle = self.source(platform_url).await?;
        Ok(handle.search_topics(topics, limit).await?)
    }

    async fn site_search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<ArchivedSearchResults> {
        let handle = self.source(query).await?;
        Ok(handle.search(query).max_results(max_results).await?)
    }
}

// ---------------------------------------------------------------------------
// SignalStore — replaces GraphWriter
// ---------------------------------------------------------------------------

#[async_trait]
pub trait SignalStore: Send + Sync {
    // --- URL/content guards ---

    /// Return the subset of `urls` that match a blocked source pattern.
    async fn blocked_urls(&self, urls: &[String]) -> Result<HashSet<String>>;

    /// Check if content with this hash has already been processed for this URL.
    async fn content_already_processed(&self, hash: &str, url: &str) -> Result<bool>;

    // --- Signal lifecycle ---

    /// Create a new signal node with embedding. Returns the node ID.
    async fn create_node(
        &self,
        node: &Node,
        embedding: &[f32],
        created_by: &str,
        run_id: &str,
    ) -> Result<Uuid>;

    /// Attach an evidence node to a signal.
    async fn create_evidence(&self, evidence: &EvidenceNode, signal_id: Uuid) -> Result<()>;

    /// Refresh a signal's last_confirmed_active timestamp (same-source re-encounter).
    async fn refresh_signal(
        &self,
        id: Uuid,
        node_type: NodeType,
        now: DateTime<Utc>,
    ) -> Result<()>;

    /// Refresh all signals from a given source URL. Returns count refreshed.
    async fn refresh_url_signals(&self, url: &str, now: DateTime<Utc>) -> Result<u64>;

    /// Increment corroboration count and recompute diversity metrics.
    async fn corroborate(
        &self,
        id: Uuid,
        node_type: NodeType,
        now: DateTime<Utc>,
        entity_mappings: &[EntityMappingOwned],
    ) -> Result<()>;

    // --- Dedup queries ---

    /// Return titles of existing signals from a given source URL.
    async fn existing_titles_for_url(&self, url: &str) -> Result<Vec<String>>;

    /// Batch-find existing signals by exact title+type. Returns map of
    /// (lowercase_title, type) → (node_id, source_url).
    async fn find_by_titles_and_types(
        &self,
        pairs: &[(String, NodeType)],
    ) -> Result<HashMap<(String, NodeType), (Uuid, String)>>;

    /// Find a duplicate signal by vector similarity within a geographic bounding box.
    async fn find_duplicate(
        &self,
        embedding: &[f32],
        primary_type: NodeType,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Option<DuplicateMatch>>;

    // --- Actor graph ---

    /// Find an actor by name (case-insensitive). Returns actor UUID if found.
    async fn find_actor_by_name(&self, name: &str) -> Result<Option<Uuid>>;

    /// Create or update an actor node.
    async fn upsert_actor(&self, actor: &ActorNode) -> Result<()>;

    /// Link an actor to a signal with a role (e.g. "mentioned", "authored").
    async fn link_actor_to_signal(
        &self,
        actor_id: Uuid,
        signal_id: Uuid,
        role: &str,
    ) -> Result<()>;

    // --- Resource graph ---

    /// Find or create a Resource node by slug. Returns the resource UUID.
    async fn find_or_create_resource(
        &self,
        name: &str,
        slug: &str,
        description: &str,
        embedding: &[f32],
    ) -> Result<Uuid>;

    /// Create a REQUIRES edge from a signal to a resource.
    async fn create_requires_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
        quantity: Option<&str>,
        notes: Option<&str>,
    ) -> Result<()>;

    /// Create a PREFERS edge from a signal to a resource.
    async fn create_prefers_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
    ) -> Result<()>;

    /// Create an OFFERS edge from a signal to a resource.
    async fn create_offers_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
        capacity: Option<&str>,
    ) -> Result<()>;

    // --- Source management ---

    /// Get all active source nodes.
    async fn get_active_sources(&self) -> Result<Vec<SourceNode>>;

    /// Create or update a source node (MERGE by canonical_key).
    async fn upsert_source(&self, source: &SourceNode) -> Result<()>;

    /// Batch-create Tag nodes and TAGGED edges for a signal.
    async fn batch_tag_signals(&self, signal_id: Uuid, tag_slugs: &[String]) -> Result<()>;
}

#[async_trait]
impl SignalStore for rootsignal_graph::GraphWriter {
    async fn blocked_urls(&self, urls: &[String]) -> Result<HashSet<String>> {
        Ok(self.blocked_urls(urls).await?)
    }

    async fn content_already_processed(&self, hash: &str, url: &str) -> Result<bool> {
        Ok(self.content_already_processed(hash, url).await?)
    }

    async fn create_node(
        &self,
        node: &Node,
        embedding: &[f32],
        created_by: &str,
        run_id: &str,
    ) -> Result<Uuid> {
        Ok(self.create_node(node, embedding, created_by, run_id).await?)
    }

    async fn create_evidence(&self, evidence: &EvidenceNode, signal_id: Uuid) -> Result<()> {
        Ok(self.create_evidence(evidence, signal_id).await?)
    }

    async fn refresh_signal(
        &self,
        id: Uuid,
        node_type: NodeType,
        now: DateTime<Utc>,
    ) -> Result<()> {
        Ok(self.refresh_signal(id, node_type, now).await?)
    }

    async fn refresh_url_signals(&self, url: &str, now: DateTime<Utc>) -> Result<u64> {
        Ok(self.refresh_url_signals(url, now).await?)
    }

    async fn corroborate(
        &self,
        id: Uuid,
        node_type: NodeType,
        now: DateTime<Utc>,
        entity_mappings: &[EntityMappingOwned],
    ) -> Result<()> {
        Ok(self.corroborate(id, node_type, now, entity_mappings).await?)
    }

    async fn existing_titles_for_url(&self, url: &str) -> Result<Vec<String>> {
        Ok(self.existing_titles_for_url(url).await?)
    }

    async fn find_by_titles_and_types(
        &self,
        pairs: &[(String, NodeType)],
    ) -> Result<HashMap<(String, NodeType), (Uuid, String)>> {
        Ok(self.find_by_titles_and_types(pairs).await?)
    }

    async fn find_duplicate(
        &self,
        embedding: &[f32],
        primary_type: NodeType,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Option<DuplicateMatch>> {
        Ok(self
            .find_duplicate(embedding, primary_type, threshold, min_lat, max_lat, min_lng, max_lng)
            .await?)
    }

    async fn find_actor_by_name(&self, name: &str) -> Result<Option<Uuid>> {
        Ok(self.find_actor_by_name(name).await?)
    }

    async fn upsert_actor(&self, actor: &ActorNode) -> Result<()> {
        Ok(self.upsert_actor(actor).await?)
    }

    async fn link_actor_to_signal(
        &self,
        actor_id: Uuid,
        signal_id: Uuid,
        role: &str,
    ) -> Result<()> {
        Ok(self.link_actor_to_signal(actor_id, signal_id, role).await?)
    }

    async fn find_or_create_resource(
        &self,
        name: &str,
        slug: &str,
        description: &str,
        embedding: &[f32],
    ) -> Result<Uuid> {
        Ok(self
            .find_or_create_resource(name, slug, description, embedding)
            .await?)
    }

    async fn create_requires_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
        quantity: Option<&str>,
        notes: Option<&str>,
    ) -> Result<()> {
        Ok(self
            .create_requires_edge(signal_id, resource_id, confidence, quantity, notes)
            .await?)
    }

    async fn create_prefers_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
    ) -> Result<()> {
        Ok(self
            .create_prefers_edge(signal_id, resource_id, confidence)
            .await?)
    }

    async fn create_offers_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
        capacity: Option<&str>,
    ) -> Result<()> {
        Ok(self
            .create_offers_edge(signal_id, resource_id, confidence, capacity)
            .await?)
    }

    async fn get_active_sources(&self) -> Result<Vec<SourceNode>> {
        Ok(self.get_active_sources().await?)
    }

    async fn upsert_source(&self, source: &SourceNode) -> Result<()> {
        Ok(self.upsert_source(source).await?)
    }

    async fn batch_tag_signals(&self, signal_id: Uuid, tag_slugs: &[String]) -> Result<()> {
        Ok(self.batch_tag_signals(signal_id, tag_slugs).await?)
    }
}
