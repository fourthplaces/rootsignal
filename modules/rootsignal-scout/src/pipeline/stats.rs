use serde::{Deserialize, Serialize};

/// Stats from a scout run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ScoutStats {
    pub urls_scraped: u32,
    pub urls_unchanged: u32,
    pub urls_failed: u32,
    pub signals_extracted: u32,
    pub signals_deduplicated: u32,
    pub signals_stored: u32,
    pub by_type: [u32; 5], // Gathering, Aid, Need, Notice, Tension
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
    pub link_failures: u32,
}

impl ScoutStats {
    /// Merge another stats snapshot into this one (additive).
    pub fn merge(&mut self, other: &ScoutStats) {
        self.urls_scraped += other.urls_scraped;
        self.urls_unchanged += other.urls_unchanged;
        self.urls_failed += other.urls_failed;
        self.signals_extracted += other.signals_extracted;
        self.signals_deduplicated += other.signals_deduplicated;
        self.signals_stored += other.signals_stored;
        for i in 0..5 {
            self.by_type[i] += other.by_type[i];
        }
        self.fresh_7d += other.fresh_7d;
        self.fresh_30d += other.fresh_30d;
        self.fresh_90d += other.fresh_90d;
        self.social_media_posts += other.social_media_posts;
        self.discovery_posts_found += other.discovery_posts_found;
        self.discovery_accounts_found += other.discovery_accounts_found;
        self.expansion_queries_collected += other.expansion_queries_collected;
        self.expansion_sources_created += other.expansion_sources_created;
        self.expansion_deferred_expanded += other.expansion_deferred_expanded;
        self.expansion_social_topics_queued += other.expansion_social_topics_queued;
        self.link_failures += other.link_failures;
    }
}

impl std::fmt::Display for ScoutStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== Scout Run Complete ===")?;
        writeln!(f, "URLs scraped:       {}", self.urls_scraped)?;
        writeln!(f, "URLs unchanged:     {}", self.urls_unchanged)?;
        writeln!(f, "URLs failed:        {}", self.urls_failed)?;
        writeln!(f, "Social media posts: {}", self.social_media_posts)?;
        writeln!(f, "Discovery posts:    {}", self.discovery_posts_found)?;
        writeln!(f, "Accounts discovered:{}", self.discovery_accounts_found)?;
        writeln!(f, "Signals extracted:  {}", self.signals_extracted)?;
        writeln!(f, "Signals deduped:    {}", self.signals_deduplicated)?;
        writeln!(f, "Signals stored:     {}", self.signals_stored)?;
        writeln!(f, "\nBy type:")?;
        writeln!(f, "  Gathering: {}", self.by_type[0])?;
        writeln!(f, "  Aid:       {}", self.by_type[1])?;
        writeln!(f, "  Need:    {}", self.by_type[2])?;
        writeln!(f, "  Notice:  {}", self.by_type[3])?;
        writeln!(f, "  Tension: {}", self.by_type[4])?;
        let total = self.signals_stored.max(1);
        writeln!(f, "\nFreshness:")?;
        writeln!(
            f,
            "  < 7 days:   {} ({:.0}%)",
            self.fresh_7d,
            self.fresh_7d as f64 / total as f64 * 100.0
        )?;
        writeln!(
            f,
            "  7-30 days:  {} ({:.0}%)",
            self.fresh_30d,
            self.fresh_30d as f64 / total as f64 * 100.0
        )?;
        writeln!(
            f,
            "  30-90 days: {} ({:.0}%)",
            self.fresh_90d,
            self.fresh_90d as f64 / total as f64 * 100.0
        )?;
        if self.link_failures > 0 {
            writeln!(f, "Link failures:      {}", self.link_failures)?;
        }
        if self.expansion_queries_collected > 0 {
            writeln!(f, "\nSignal expansion:")?;
            writeln!(
                f,
                "  Queries collected: {}",
                self.expansion_queries_collected
            )?;
            writeln!(f, "  Sources created:   {}", self.expansion_sources_created)?;
            writeln!(
                f,
                "  Deferred expanded: {}",
                self.expansion_deferred_expanded
            )?;
            if self.expansion_social_topics_queued > 0 {
                writeln!(
                    f,
                    "  Social topics:     {}",
                    self.expansion_social_topics_queued
                )?;
            }
        }
        Ok(())
    }
}
