use std::sync::atomic::{AtomicU64, Ordering};

/// Simple per-run budget tracker for LLM calls.
/// Tracks the number of LLM checks performed and enforces a cap.
pub struct BudgetTracker {
    max_checks: u64,
    checks_used: AtomicU64,
}

impl BudgetTracker {
    pub fn new(max_checks: u64) -> Self {
        Self {
            max_checks,
            checks_used: AtomicU64::new(0),
        }
    }

    /// Try to consume one check. Returns true if within budget.
    pub fn try_consume(&self) -> bool {
        let current = self.checks_used.fetch_add(1, Ordering::Relaxed);
        if current >= self.max_checks {
            self.checks_used.fetch_sub(1, Ordering::Relaxed);
            return false;
        }
        true
    }

    pub fn used(&self) -> u64 {
        self.checks_used.load(Ordering::Relaxed)
    }

    pub fn remaining(&self) -> u64 {
        self.max_checks.saturating_sub(self.used())
    }
}
