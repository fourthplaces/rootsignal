use chrono::{DateTime, Utc};
use std::fmt;
use uuid::Uuid;

/// A validation issue found by the supervisor.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub id: Uuid,
    pub city: String,
    pub issue_type: IssueType,
    pub severity: Severity,
    pub target_id: Uuid,
    pub target_label: String,
    pub description: String,
    pub suggested_action: String,
    pub status: IssueStatus,
    pub created_at: DateTime<Utc>,
}

impl ValidationIssue {
    pub fn new(
        city: &str,
        issue_type: IssueType,
        severity: Severity,
        target_id: Uuid,
        target_label: &str,
        description: String,
        suggested_action: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            city: city.to_string(),
            issue_type,
            severity,
            target_id,
            target_label: target_label.to_string(),
            description,
            suggested_action,
            status: IssueStatus::Open,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueType {
    Misclassification,
    IncoherentStory,
    BadRespondsTo,
    NearDuplicate,
    LowConfidenceHighVisibility,
}

impl fmt::Display for IssueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Misclassification => write!(f, "misclassification"),
            Self::IncoherentStory => write!(f, "incoherent_story"),
            Self::BadRespondsTo => write!(f, "bad_responds_to"),
            Self::NearDuplicate => write!(f, "near_duplicate"),
            Self::LowConfidenceHighVisibility => write!(f, "low_confidence_high_visibility"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueStatus {
    Open,
    Resolved,
    Dismissed,
}

impl fmt::Display for IssueStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Resolved => write!(f, "resolved"),
            Self::Dismissed => write!(f, "dismissed"),
        }
    }
}

/// Stats from a supervisor run.
#[derive(Debug, Default)]
pub struct SupervisorStats {
    pub auto_fix: AutoFixStats,
    pub signals_checked: u64,
    pub stories_checked: u64,
    pub issues_created: u64,
    pub sources_penalized: u64,
    pub sources_reset: u64,
    pub echoes_flagged: u64,
}

impl fmt::Display for SupervisorStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "signals_checked={} stories_checked={} issues_created={} sources_penalized={} sources_reset={} echoes_flagged={} {}",
            self.signals_checked, self.stories_checked, self.issues_created,
            self.sources_penalized, self.sources_reset, self.echoes_flagged, self.auto_fix,
        )
    }
}

/// Stats from auto-fix checks.
#[derive(Debug, Default)]
pub struct AutoFixStats {
    pub orphaned_evidence_deleted: u64,
    pub orphaned_edges_deleted: u64,
    pub actors_merged: u64,
    pub empty_signals_deleted: u64,
    pub fake_coords_nulled: u64,
}

impl fmt::Display for AutoFixStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "auto_fix(orphaned_evidence={} orphaned_edges={} actors_merged={} empty_signals={} fake_coords={})",
            self.orphaned_evidence_deleted,
            self.orphaned_edges_deleted,
            self.actors_merged,
            self.empty_signals_deleted,
            self.fake_coords_nulled,
        )
    }
}
