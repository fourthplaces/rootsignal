use std::collections::HashMap;

use rootsignal_common::types::NodeType;
use serde::{Deserialize, Serialize};

/// Stats from a scout run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoutStats {
    pub urls_scraped: u32,
    pub urls_unchanged: u32,
    pub urls_failed: u32,
    pub signals_extracted: u32,
    pub signals_rejected: u32,
    pub signals_deduplicated: u32,
    pub signals_stored: u32,
    pub by_type: HashMap<NodeType, u32>,
    pub fresh_7d: u32,
    pub fresh_30d: u32,
    pub fresh_90d: u32,
    pub social_media_posts: u32,
    pub discovery_posts_found: u32,
    pub discovery_accounts_found: u32,
    pub expansion_queries_collected: u32,
    pub expansion_sources_created: u32,
    pub expansion_deferred_expanded: u32,
    pub expansion_social_topics_queued: u32,
    pub sources_discovered: u32,
    pub signals_updated: u32,
    pub link_failures: u32,
    pub handler_failures: u32,
    pub spent_cents: u64,
}

impl ScoutStats {
    /// Merge another stats snapshot into this one (additive).
    ///
    /// Destructures `other` so the compiler forces you to handle new fields.
    pub fn merge(&mut self, other: &ScoutStats) {
        let ScoutStats {
            urls_scraped,
            urls_unchanged,
            urls_failed,
            signals_extracted,
            signals_rejected,
            signals_deduplicated,
            signals_stored,
            ref by_type,
            fresh_7d,
            fresh_30d,
            fresh_90d,
            social_media_posts,
            discovery_posts_found,
            discovery_accounts_found,
            expansion_queries_collected,
            expansion_sources_created,
            expansion_deferred_expanded,
            expansion_social_topics_queued,
            sources_discovered,
            signals_updated,
            link_failures,
            handler_failures,
            spent_cents,
        } = *other;

        self.urls_scraped += urls_scraped;
        self.urls_unchanged += urls_unchanged;
        self.urls_failed += urls_failed;
        self.signals_extracted += signals_extracted;
        self.signals_rejected += signals_rejected;
        self.signals_deduplicated += signals_deduplicated;
        self.signals_stored += signals_stored;
        for (nt, count) in by_type {
            *self.by_type.entry(*nt).or_default() += count;
        }
        self.fresh_7d += fresh_7d;
        self.fresh_30d += fresh_30d;
        self.fresh_90d += fresh_90d;
        self.social_media_posts += social_media_posts;
        self.discovery_posts_found += discovery_posts_found;
        self.discovery_accounts_found += discovery_accounts_found;
        self.expansion_queries_collected += expansion_queries_collected;
        self.expansion_sources_created += expansion_sources_created;
        self.expansion_deferred_expanded += expansion_deferred_expanded;
        self.expansion_social_topics_queued += expansion_social_topics_queued;
        self.sources_discovered += sources_discovered;
        self.signals_updated += signals_updated;
        self.link_failures += link_failures;
        self.handler_failures += handler_failures;
        self.spent_cents += spent_cents;
    }
}

impl std::fmt::Display for ScoutStats {
    /// Destructures self so the compiler forces you to handle new fields.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ScoutStats {
            urls_scraped,
            urls_unchanged,
            urls_failed,
            signals_extracted,
            signals_rejected,
            signals_deduplicated,
            signals_stored,
            ref by_type,
            fresh_7d,
            fresh_30d,
            fresh_90d,
            social_media_posts,
            discovery_posts_found,
            discovery_accounts_found,
            expansion_queries_collected,
            expansion_sources_created,
            expansion_deferred_expanded,
            expansion_social_topics_queued,
            sources_discovered,
            signals_updated,
            link_failures,
            handler_failures,
            spent_cents,
        } = *self;

        writeln!(f, "\n=== Scout Run Complete ===")?;
        writeln!(f, "URLs scraped:       {urls_scraped}")?;
        writeln!(f, "URLs unchanged:     {urls_unchanged}")?;
        writeln!(f, "URLs failed:        {urls_failed}")?;
        writeln!(f, "Social media posts: {social_media_posts}")?;
        writeln!(f, "Discovery posts:    {discovery_posts_found}")?;
        writeln!(f, "Accounts discovered:{discovery_accounts_found}")?;
        writeln!(f, "Signals extracted:  {signals_extracted}")?;
        if signals_rejected > 0 {
            writeln!(f, "Signals rejected:   {signals_rejected}")?;
        }
        writeln!(f, "Signals deduped:    {signals_deduplicated}")?;
        writeln!(f, "Signals stored:     {signals_stored}")?;
        if signals_updated > 0 {
            writeln!(f, "Signals updated:    {signals_updated}")?;
        }
        if !by_type.is_empty() {
            writeln!(f, "\nBy type:")?;
            let mut entries: Vec<_> = by_type.iter().collect();
            entries.sort_by_key(|(nt, _)| format!("{nt}"));
            for (nt, count) in entries {
                writeln!(f, "  {nt}: {count}")?;
            }
        }
        let total = signals_stored.max(1);
        writeln!(f, "\nFreshness:")?;
        writeln!(
            f,
            "  < 7 days:   {fresh_7d} ({:.0}%)",
            fresh_7d as f64 / total as f64 * 100.0
        )?;
        writeln!(
            f,
            "  7-30 days:  {fresh_30d} ({:.0}%)",
            fresh_30d as f64 / total as f64 * 100.0
        )?;
        writeln!(
            f,
            "  30-90 days: {fresh_90d} ({:.0}%)",
            fresh_90d as f64 / total as f64 * 100.0
        )?;
        if sources_discovered > 0 {
            writeln!(f, "Sources discovered: {sources_discovered}")?;
        }
        if link_failures > 0 {
            writeln!(f, "Link failures:      {link_failures}")?;
        }
        if handler_failures > 0 {
            writeln!(f, "Handler failures:   {handler_failures}")?;
        }
        if expansion_queries_collected > 0 {
            writeln!(f, "\nSignal expansion:")?;
            writeln!(f, "  Queries collected: {expansion_queries_collected}")?;
            writeln!(f, "  Sources created:   {expansion_sources_created}")?;
            writeln!(f, "  Deferred expanded: {expansion_deferred_expanded}")?;
            if expansion_social_topics_queued > 0 {
                writeln!(f, "  Social topics:     {expansion_social_topics_queued}")?;
            }
        }
        if spent_cents > 0 {
            writeln!(f, "Budget spent:       {spent_cents} cents")?;
        }
        Ok(())
    }
}
