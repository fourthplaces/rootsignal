// Trait abstractions for Scraper dependencies.
//
// ContentFetcher replaces Arc<Archive> — all content fetching behind one trait.
// SignalReader — read-only graph queries (dedup, actors, sources) plus infra ops.
//   All domain writes flow through the engine dispatch loop.
//
// These enable deterministic testing with MockFetcher and MockSignalReader:
// no network, no database, no Docker. `cargo test` in seconds.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::types::{
    ActorNode, ArchivedFeed, ArchivedPage, ArchivedSearchResults, NodeType, Post, ProfileSnapshot,
    ShortVideo, SocialPlatform, SourceNode, Story,
};

/// Result of a fingerprint match against the graph.
#[derive(Debug, Clone)]
pub struct FingerprintMatch {
    pub id: Uuid,
    pub canonical_key: String,
    pub embedding: Option<Vec<f32>>,
}

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
    async fn site_search(&self, query: &str, max_results: usize) -> Result<ArchivedSearchResults>;

    /// Fetch stories (ephemeral content) for an account.
    async fn stories(&self, identifier: &str) -> Result<Vec<Story>>;

    /// Fetch short-form videos (reels, TikToks) for an account.
    async fn short_videos(&self, identifier: &str, limit: u32) -> Result<Vec<ShortVideo>>;

    /// Fetch profile metadata (bio, external URL) for a social account.
    async fn profile(&self, identifier: &str, platform: SocialPlatform) -> Result<Option<ProfileSnapshot>>;
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

    async fn site_search(&self, query: &str, max_results: usize) -> Result<ArchivedSearchResults> {
        let handle = self.source(query).await?;
        Ok(handle.search(query).max_results(max_results).await?)
    }

    async fn stories(&self, identifier: &str) -> Result<Vec<Story>> {
        let handle = self.source(identifier).await?;
        Ok(handle.stories().await?)
    }

    async fn short_videos(&self, identifier: &str, limit: u32) -> Result<Vec<ShortVideo>> {
        let handle = self.source(identifier).await?;
        Ok(handle.short_videos(limit).await?)
    }

    async fn profile(&self, identifier: &str, platform: SocialPlatform) -> Result<Option<ProfileSnapshot>> {
        let handle = self.source(identifier).await?;
        Ok(handle.profile(platform).await?)
    }
}

// ---------------------------------------------------------------------------
// SignalReader — read-only graph queries
// ---------------------------------------------------------------------------

#[async_trait]
pub trait SignalReader: Send + Sync {
    // --- URL/content guards ---

    /// Return the subset of `urls` that match a blocked source pattern.
    async fn blocked_urls(&self, urls: &[String]) -> Result<HashSet<String>>;

    /// Check if content with this hash has already been processed for this URL.
    async fn content_already_processed(&self, hash: &str, url: &str) -> Result<bool>;

    // --- Signal queries ---

    /// Return signal IDs grouped by type for a given source URL.
    async fn signal_ids_for_url(&self, url: &str) -> Result<Vec<(Uuid, NodeType)>>;

    // --- Dedup queries ---

    /// Batch-find existing signals by exact (url, node_type) fingerprint.
    async fn find_by_fingerprints(
        &self,
        pairs: &[(String, NodeType)],
    ) -> Result<HashMap<(String, NodeType), FingerprintMatch>>;

    // --- Actor graph ---

    /// Find an actor by canonical_key (URL-based identity).
    async fn find_actor_by_canonical_key(&self, canonical_key: &str) -> Result<Option<Uuid>>;

    // --- Source management ---

    /// Get all active source nodes.
    async fn get_active_sources(&self) -> Result<Vec<SourceNode>>;

    /// Find a source by its canonical_key.
    async fn find_source_by_canonical_key(&self, canonical_key: &str) -> Result<Option<Uuid>>;

    // --- Signal reaping ---

    /// Find signals that have expired by age or staleness rules.
    /// Returns (signal_id, node_type, reason) tuples for the caller to act on.
    async fn find_expired_signals(&self) -> Result<Vec<(Uuid, NodeType, String)>>;

    // --- Actor location enrichment ---

    /// Get signal location observations for an actor (authored signals with about_location).
    /// Returns (lat, lng, location_name, extracted_at) tuples.
    async fn get_signals_for_actor(
        &self,
        actor_id: Uuid,
    ) -> Result<Vec<(f64, f64, String, DateTime<Utc>)>>;

    /// List all actors with their linked sources.
    async fn list_all_actors(&self) -> Result<Vec<(ActorNode, Vec<SourceNode>)>>;
}

/// No-op SignalReader for infrastructure-only engines that never call read methods.
pub struct NoOpSignalReader;

#[async_trait]
impl SignalReader for NoOpSignalReader {
    async fn blocked_urls(&self, _: &[String]) -> Result<HashSet<String>> { Ok(HashSet::new()) }
    async fn content_already_processed(&self, _: &str, _: &str) -> Result<bool> { Ok(false) }
    async fn signal_ids_for_url(&self, _: &str) -> Result<Vec<(Uuid, NodeType)>> { Ok(vec![]) }
    async fn find_by_fingerprints(&self, _: &[(String, NodeType)]) -> Result<HashMap<(String, NodeType), FingerprintMatch>> { Ok(HashMap::new()) }
    async fn find_actor_by_canonical_key(&self, _: &str) -> Result<Option<Uuid>> { Ok(None) }
    async fn get_active_sources(&self) -> Result<Vec<SourceNode>> { Ok(vec![]) }
    async fn find_source_by_canonical_key(&self, _: &str) -> Result<Option<Uuid>> { Ok(None) }
    async fn find_expired_signals(&self) -> Result<Vec<(Uuid, NodeType, String)>> { Ok(vec![]) }
    async fn get_signals_for_actor(&self, _: Uuid) -> Result<Vec<(f64, f64, String, DateTime<Utc>)>> { Ok(vec![]) }
    async fn list_all_actors(&self) -> Result<Vec<(ActorNode, Vec<SourceNode>)>> { Ok(vec![]) }
}

