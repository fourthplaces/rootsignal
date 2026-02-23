/// Stats from a scout run.
#[derive(Debug, Default)]
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
    pub geo_filtered: u32,
    pub discovery_posts_found: u32,
    pub discovery_accounts_found: u32,
    pub expansion_queries_collected: u32,
    pub expansion_sources_created: u32,
    pub expansion_deferred_expanded: u32,
    pub expansion_social_topics_queued: u32,
}

impl std::fmt::Display for ScoutStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== Scout Run Complete ===")?;
        writeln!(f, "URLs scraped:       {}", self.urls_scraped)?;
        writeln!(f, "URLs unchanged:     {}", self.urls_unchanged)?;
        writeln!(f, "URLs failed:        {}", self.urls_failed)?;
        writeln!(f, "Social media posts: {}", self.social_media_posts)?;
        writeln!(f, "Geo filtered:       {}", self.geo_filtered)?;
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
