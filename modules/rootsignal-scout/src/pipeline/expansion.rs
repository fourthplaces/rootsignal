//! Signal expansion stage.
//!
//! Collects implied queries from extracted signals (both immediate and deferred
//! from response mapping), deduplicates them via Jaccard + embedding similarity,
//! and creates new WebQuery sources for future scout runs.

use std::collections::HashSet;

use tracing::{info, warn};
use rootsignal_common::{canonical_value, DiscoveryMethod, SourceNode};
use rootsignal_graph::GraphWriter;

use crate::infra::embedder::TextEmbedder;
use crate::infra::run_log::{EventKind, RunLog};
use crate::pipeline::scrape_phase::RunContext;

// --- Constants ---

const DEDUP_JACCARD_THRESHOLD: f64 = 0.6;
const MAX_EXPANSION_QUERIES_PER_RUN: usize = 10;
const MAX_EXPANSION_SOCIAL_TOPICS: usize = 5;

// --- Expansion stage ---

pub(crate) struct Expansion<'a> {
    writer: &'a GraphWriter,
    store: &'a dyn crate::pipeline::traits::SignalStore,
    embedder: &'a dyn TextEmbedder,
    region_slug: &'a str,
}

impl<'a> Expansion<'a> {
    pub fn new(
        writer: &'a GraphWriter,
        store: &'a dyn crate::pipeline::traits::SignalStore,
        embedder: &'a dyn TextEmbedder,
        region_slug: &'a str,
    ) -> Self {
        Self {
            writer,
            store,
            embedder,
            region_slug,
        }
    }

    /// Run the expansion stage:
    /// 1. Collect deferred expansion queries (from recently linked signals)
    /// 2. Deduplicate against existing WebQuery sources (Jaccard + embedding)
    /// 3. Create new WebQuery sources for surviving queries
    pub async fn run(&self, ctx: &mut RunContext, run_log: &mut RunLog) {
        // Deferred expansion: collect implied queries from Give/Event signals
        // that are now linked to tensions via response mapping.
        match self
            .writer
            .get_recently_linked_signals_with_queries()
            .await
        {
            Ok(deferred) => {
                let deferred_count = deferred.len();
                ctx.expansion_queries.extend(deferred);
                if deferred_count > 0 {
                    info!(
                        deferred = deferred_count,
                        "Deferred signal expansion queries collected"
                    );
                }
                ctx.stats.expansion_deferred_expanded = deferred_count as u32;
            }
            Err(e) => warn!(error = %e, "Failed to get deferred expansion queries"),
        }

        ctx.stats.expansion_queries_collected = ctx.expansion_queries.len() as u32;

        if ctx.expansion_queries.is_empty() {
            return;
        }

        let existing = self
            .writer
            .get_active_web_queries()
            .await
            .unwrap_or_default();
        let deduped: Vec<String> = ctx
            .expansion_queries
            .iter()
            .filter(|q| {
                !existing
                    .iter()
                    .any(|e| jaccard_similarity(q, e) > DEDUP_JACCARD_THRESHOLD)
            })
            .cloned()
            .take(MAX_EXPANSION_QUERIES_PER_RUN)
            .collect();

        let mut created = 0u32;
        let mut expansion_dupes_skipped = 0u32;
        for query_text in &deduped {
            // Embedding-based dedup for expansion queries
            if let Ok(embedding) = self.embedder.embed(query_text).await {
                match self
                    .writer
                    .find_similar_query(&embedding, 0.90)
                    .await
                {
                    Ok(Some((existing_ck, sim))) => {
                        info!(
                            query = query_text.as_str(),
                            existing_key = existing_ck.as_str(),
                            similarity = format!("{sim:.3}").as_str(),
                            "Skipping semantically duplicate expansion query"
                        );
                        expansion_dupes_skipped += 1;
                        continue;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!(error = %e, "Expansion query dedup check failed, proceeding")
                    }
                }
            }

            let cv = query_text.clone();
            let ck = canonical_value(&cv);
            let mut source = SourceNode::new(
                ck.clone(),
                cv,
                None,
                DiscoveryMethod::SignalExpansion,
                crate::discovery::source_finder::initial_weight_for_method(
                    DiscoveryMethod::SignalExpansion,
                    None,
                ),
                rootsignal_common::SourceRole::Response,
                Some("Signal expansion: implied query from extracted signal".to_string()),
            );
            match self.store.upsert_source(&source).await {
                Ok(_) => {
                    run_log.log(EventKind::ExpansionSourceCreated {
                        canonical_key: ck.clone(),
                        query: query_text.clone(),
                    });
                    created += 1;
                    // Store embedding for future dedup
                    if let Ok(embedding) = self.embedder.embed(query_text).await {
                        if let Err(e) = self.writer.set_query_embedding(&ck, &embedding).await {
                            warn!(error = %e, "Failed to store expansion query embedding (non-fatal)");
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, query = query_text.as_str(), "Failed to create expansion source")
                }
            }
        }
        ctx.stats.expansion_sources_created = created;

        // Social expansion: route deduped queries as social topics too.
        // This creates the social flywheel — expansion from social-sourced
        // tensions stays in the social channel instead of always going web.
        let social_count = deduped.len().min(MAX_EXPANSION_SOCIAL_TOPICS);
        ctx.social_expansion_topics
            .extend(deduped[..social_count].iter().cloned());
        ctx.stats.expansion_social_topics_queued = social_count as u32;

        info!(
            collected = ctx.expansion_queries.len(),
            created,
            deferred = ctx.stats.expansion_deferred_expanded,
            embedding_dupes = expansion_dupes_skipped,
            social_topics = social_count,
            "Signal expansion complete"
        );
    }
}

// --- Helpers ---

/// Token-based Jaccard similarity for query dedup.
/// Uses word overlap rather than substring matching to preserve specific long-tail queries.
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let a_tokens: HashSet<&str> = a_lower.split_whitespace().collect();
    let b_tokens: HashSet<&str> = b_lower.split_whitespace().collect();
    let intersection = a_tokens.intersection(&b_tokens).count();
    let union = a_tokens.union(&b_tokens).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_specific_vs_generic_passes() {
        let sim = jaccard_similarity("emergency housing for detained immigrants", "housing");
        assert!(
            sim < DEDUP_JACCARD_THRESHOLD,
            "Specific long-tail query should not match generic: {sim}"
        );
    }

    #[test]
    fn jaccard_similar_queries_blocked() {
        let sim = jaccard_similarity(
            "housing assistance programs Minneapolis",
            "housing assistance resources Minneapolis",
        );
        assert!(
            sim >= DEDUP_JACCARD_THRESHOLD,
            "Similar queries should be flagged as duplicate: {sim}"
        );
    }

    #[test]
    fn jaccard_identical_blocked() {
        let sim = jaccard_similarity(
            "immigration legal aid Minneapolis",
            "immigration legal aid Minneapolis",
        );
        assert!(
            (sim - 1.0).abs() < f64::EPSILON,
            "Identical queries should have Jaccard 1.0: {sim}"
        );
    }

    #[test]
    fn jaccard_empty_strings() {
        assert_eq!(jaccard_similarity("", ""), 0.0);
        assert_eq!(jaccard_similarity("hello", ""), 0.0);
    }

    #[test]
    fn jaccard_case_insensitive() {
        let sim = jaccard_similarity("Housing Minneapolis", "housing minneapolis");
        assert!(
            (sim - 1.0).abs() < f64::EPSILON,
            "Jaccard should be case-insensitive: {sim}"
        );
    }

    #[test]
    fn max_expansion_queries_constant() {
        assert_eq!(MAX_EXPANSION_QUERIES_PER_RUN, 10);
    }

    #[test]
    fn dedup_threshold_constant() {
        assert!((DEDUP_JACCARD_THRESHOLD - 0.6).abs() < f64::EPSILON);
    }
}
