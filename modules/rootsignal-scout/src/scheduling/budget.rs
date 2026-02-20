use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn};

/// Tracks spend against a daily budget limit.
/// Thread-safe via atomic operations for concurrent scraping.
pub struct BudgetTracker {
    /// Daily limit in cents. 0 = unlimited.
    daily_limit_cents: u64,
    /// Cumulative spend this run in cents.
    spent_cents: AtomicU64,
}

/// Estimated cost per operation in cents.
pub struct OperationCost;

impl OperationCost {
    pub const SEARCH_QUERY: u64 = 1;
    pub const CHROME_SCRAPE: u64 = 1; // ~0.5 but round up
    pub const APIFY_SOCIAL: u64 = 1;
    pub const CLAUDE_HAIKU_EXTRACTION: u64 = 1; // ~0.2 but round up
    pub const VOYAGE_EMBEDDING: u64 = 1; // ~0.01 per batch, round up
    pub const CLAUDE_HAIKU_SYNTHESIS: u64 = 1; // ~0.5 per story
    pub const CLAUDE_HAIKU_INVESTIGATION: u64 = 1; // ~0.1 per query gen + ~1 per search
    pub const SEARCH_INVESTIGATION: u64 = 1;
    pub const CLAUDE_HAIKU_DISCOVERY: u64 = 1; // ~0.2 per discovery briefing
    pub const CLAUDE_HAIKU_TENSION_LINKER: u64 = 2; // per signal: agentic investigation + structuring
    pub const SEARCH_TENSION_LINKER: u64 = 3; // per signal: up to 3 searches
    pub const CHROME_TENSION_LINKER: u64 = 2; // per signal: up to 2 page reads
    pub const CLAUDE_HAIKU_STORY_WEAVE: u64 = 1; // per story: synthesis enrichment
    pub const CLAUDE_HAIKU_RESPONSE_FINDER: u64 = 3; // per tension: investigation + structuring
    pub const SEARCH_RESPONSE_FINDER: u64 = 5; // per tension: up to 5 searches
    pub const CHROME_RESPONSE_FINDER: u64 = 3; // per tension: page reads
    pub const CLAUDE_HAIKU_ACTOR_EXTRACTOR: u64 = 1; // per batch: actor extraction from signal text
    pub const CLAUDE_HAIKU_GATHERING_FINDER: u64 = 3; // per tension: investigation + extraction (may terminate early)
    pub const SEARCH_GATHERING_FINDER: u64 = 5; // per tension: early termination uses ~2-3
    pub const CHROME_GATHERING_FINDER: u64 = 3; // per tension: page reads
}

impl BudgetTracker {
    pub fn new(daily_limit_cents: u64) -> Self {
        Self {
            daily_limit_cents,
            spent_cents: AtomicU64::new(0),
        }
    }

    /// Check if there's budget remaining for an operation.
    pub fn has_budget(&self, cost_cents: u64) -> bool {
        if self.daily_limit_cents == 0 {
            return true; // Unlimited
        }
        self.spent_cents.load(Ordering::Relaxed) + cost_cents <= self.daily_limit_cents
    }

    /// Record spend. Returns false if budget would be exceeded (spend is still recorded).
    pub fn spend(&self, cost_cents: u64) -> bool {
        let prev = self.spent_cents.fetch_add(cost_cents, Ordering::Relaxed);
        if self.daily_limit_cents > 0 && prev + cost_cents > self.daily_limit_cents {
            warn!(
                spent = prev + cost_cents,
                limit = self.daily_limit_cents,
                "Budget exceeded"
            );
            return false;
        }
        true
    }

    /// Total spent this run.
    pub fn total_spent(&self) -> u64 {
        self.spent_cents.load(Ordering::Relaxed)
    }

    /// Budget remaining (0 if unlimited or exhausted).
    pub fn remaining(&self) -> u64 {
        if self.daily_limit_cents == 0 {
            return u64::MAX;
        }
        self.daily_limit_cents
            .saturating_sub(self.spent_cents.load(Ordering::Relaxed))
    }

    /// Whether budget tracking is active (limit > 0).
    pub fn is_active(&self) -> bool {
        self.daily_limit_cents > 0
    }

    /// Log budget status.
    pub fn log_status(&self) {
        if self.is_active() {
            let spent = self.total_spent();
            let remaining = self.remaining();
            info!(
                spent_cents = spent,
                remaining_cents = remaining,
                limit_cents = self.daily_limit_cents,
                "Budget status"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_budget_always_has_budget() {
        let tracker = BudgetTracker::new(0);
        assert!(tracker.has_budget(1000));
        assert!(tracker.spend(1000));
        assert!(!tracker.is_active());
    }

    #[test]
    fn budget_tracks_spend() {
        let tracker = BudgetTracker::new(100);
        assert!(tracker.has_budget(50));
        assert!(tracker.spend(50));
        assert_eq!(tracker.total_spent(), 50);
        assert_eq!(tracker.remaining(), 50);
    }

    #[test]
    fn budget_exceeded_returns_false() {
        let tracker = BudgetTracker::new(100);
        assert!(tracker.spend(80));
        assert!(!tracker.has_budget(30));
        assert!(!tracker.spend(30)); // Still records but returns false
        assert_eq!(tracker.total_spent(), 110);
    }
}
