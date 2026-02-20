use anyhow::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};

use ai_client::claude::Claude;

use super::triage::{Suspect, SuspectType};
use crate::budget::BudgetTracker;
use crate::types::{IssueType, Severity, ValidationIssue};

const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";
const SONNET_MODEL: &str = "claude-sonnet-4-5-20250929";

// =============================================================================
// Structured output types for LLM checks
// =============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
struct MisclassificationResult {
    /// Whether the signal is correctly classified
    is_correct: bool,
    /// If misclassified, the correct type (Gathering, Aid, Need, Notice, Tension)
    suggested_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CoherenceResult {
    /// Whether the signals form a coherent story
    is_coherent: bool,
    /// If incoherent, explanation of why signals don't belong together
    explanation: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RespondsToResult {
    /// Whether the response genuinely addresses the tension
    is_valid: bool,
    /// If invalid, explanation of why the match is wrong
    explanation: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DuplicateResult {
    /// Whether the two signals describe the same real-world thing
    is_duplicate: bool,
}

// =============================================================================
// Main check function
// =============================================================================

/// Run LLM-powered checks on triaged suspects. Respects the budget cap.
/// Returns a list of ValidationIssues for confirmed problems.
pub async fn check_suspects(
    suspects: Vec<Suspect>,
    api_key: &str,
    city: &str,
    budget: &BudgetTracker,
) -> Vec<ValidationIssue> {
    let haiku = Claude::new(api_key, HAIKU_MODEL);
    let sonnet = Claude::new(api_key, SONNET_MODEL);

    let mut issues = Vec::new();

    for suspect in suspects {
        if !budget.try_consume() {
            info!(
                remaining = budget.remaining(),
                used = budget.used(),
                "LLM budget exhausted, skipping remaining suspects"
            );
            break;
        }

        let result = match suspect.check_type {
            SuspectType::Misclassification => check_misclassification(&haiku, &suspect).await,
            SuspectType::IncoherentStory => check_incoherent_story(&sonnet, &suspect).await,
            SuspectType::BadRespondsTo => check_bad_responds_to(&sonnet, &suspect).await,
            SuspectType::NearDuplicate => check_near_duplicate(&haiku, &suspect).await,
            SuspectType::LowConfidenceHighVisibility => {
                // No LLM check needed — the heuristic itself is the flag
                Ok(Some(ValidationIssue::new(
                    city,
                    IssueType::LowConfidenceHighVisibility,
                    Severity::Warning,
                    suspect.id,
                    &suspect.label,
                    format!(
                        "Signal '{}' has very low confidence but appears in a confirmed story. {}",
                        suspect.title, suspect.context,
                    ),
                    "Review signal quality. Consider removing from story or improving evidence."
                        .to_string(),
                )))
            }
        };

        match result {
            Ok(Some(issue)) => {
                info!(
                    issue_type = %issue.issue_type,
                    target = %issue.target_id,
                    "Issue confirmed by LLM"
                );
                issues.push(issue);
            }
            Ok(None) => {
                // LLM determined this is not an issue
            }
            Err(e) => {
                warn!(
                    error = %e,
                    suspect_id = %suspect.id,
                    check_type = ?suspect.check_type,
                    "LLM check failed"
                );
            }
        }
    }

    info!(confirmed = issues.len(), "LLM checks complete");
    issues
}

async fn check_misclassification(
    claude: &Claude,
    suspect: &Suspect,
) -> Result<Option<ValidationIssue>> {
    let system = "You are a signal classifier. Given a signal's title, summary, and source evidence, \
        determine if it is correctly classified. Signal types are: Gathering (time-bound gathering), \
        Aid (available resource/service), Need (community need requesting help), \
        Notice (official advisory/policy), Tension (community conflict or systemic problem).";

    let user = format!(
        "Signal title: {}\nSignal summary: {}\n\n{}",
        suspect.title, suspect.summary, suspect.context,
    );

    let result: MisclassificationResult = claude.extract(HAIKU_MODEL, system, &user).await?;

    if !result.is_correct {
        let suggested_type = result.suggested_type.as_deref().unwrap_or("unknown");
        Ok(Some(ValidationIssue::new(
            "", // city set by caller
            IssueType::Misclassification,
            Severity::Warning,
            suspect.id,
            &suspect.label,
            format!(
                "Signal '{}' appears to be misclassified as {}. LLM suggests it should be {}.",
                suspect.title, suspect.label, suggested_type,
            ),
            format!("Review and reclassify as {suggested_type} if confirmed."),
        )))
    } else {
        Ok(None)
    }
}

async fn check_incoherent_story(
    claude: &Claude,
    suspect: &Suspect,
) -> Result<Option<ValidationIssue>> {
    let system = "You are a narrative coherence reviewer. Given a story headline and its constituent \
        signals, determine if they form a coherent narrative about a single topic or related set of events. \
        If incoherent, identify which signals (by their title) seem out of place.";

    let user = format!(
        "Story headline: {}\nStory summary: {}\n\n{}",
        suspect.title, suspect.summary, suspect.context,
    );

    let result: CoherenceResult = claude.extract(SONNET_MODEL, system, &user).await?;

    if !result.is_coherent {
        let explanation = result.explanation.unwrap_or_default();
        Ok(Some(ValidationIssue::new(
            "",
            IssueType::IncoherentStory,
            Severity::Warning,
            suspect.id,
            "Story",
            format!(
                "Story '{}' contains signals that don't form a coherent narrative. {}",
                suspect.title, explanation,
            ),
            "Review story clustering. Consider splitting into separate stories or removing misfit signals.".to_string(),
        )))
    } else {
        Ok(None)
    }
}

async fn check_bad_responds_to(
    claude: &Claude,
    suspect: &Suspect,
) -> Result<Option<ValidationIssue>> {
    let system = "You are reviewing whether a community resource or event genuinely addresses a \
        community tension or need.";

    let user = format!(
        "Signal: {} — {}\n\n{}",
        suspect.title, suspect.summary, suspect.context,
    );

    let result: RespondsToResult = claude.extract(SONNET_MODEL, system, &user).await?;

    if !result.is_valid {
        let explanation = result.explanation.unwrap_or_default();
        Ok(Some(ValidationIssue::new(
            "",
            IssueType::BadRespondsTo,
            Severity::Warning,
            suspect.id,
            &suspect.label,
            format!(
                "RESPONDS_TO edge from '{}' does not genuinely address the linked tension. {}",
                suspect.title, explanation,
            ),
            "Review and remove the RESPONDS_TO edge if confirmed.".to_string(),
        )))
    } else {
        Ok(None)
    }
}

async fn check_near_duplicate(
    claude: &Claude,
    suspect: &Suspect,
) -> Result<Option<ValidationIssue>> {
    let system = "You are checking whether two signals describe the same real-world thing. \
        They may use different words but refer to the same event, resource, need, or issue.";

    let result: DuplicateResult = claude.extract(HAIKU_MODEL, system, &suspect.context).await?;

    if result.is_duplicate {
        Ok(Some(ValidationIssue::new(
            "",
            IssueType::NearDuplicate,
            Severity::Info,
            suspect.id,
            &suspect.label,
            format!(
                "Signal '{}' appears to be a near-duplicate of another signal. {}",
                suspect.title, suspect.context,
            ),
            "Review and corroborate (merge) if confirmed.".to_string(),
        )))
    } else {
        Ok(None)
    }
}
