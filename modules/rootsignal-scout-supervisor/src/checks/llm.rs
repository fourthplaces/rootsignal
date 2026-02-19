use anyhow::Result;
use tracing::{info, warn};

use ai_client::claude::Claude;

use crate::budget::BudgetTracker;
use crate::types::{IssueType, Severity, ValidationIssue};
use super::triage::{Suspect, SuspectType};

const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";
const SONNET_MODEL: &str = "claude-sonnet-4-5-20250929";

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
            SuspectType::Misclassification => {
                check_misclassification(&haiku, &suspect).await
            }
            SuspectType::IncoherentStory => {
                check_incoherent_story(&sonnet, &suspect).await
            }
            SuspectType::BadRespondsTo => {
                check_bad_responds_to(&sonnet, &suspect).await
            }
            SuspectType::NearDuplicate => {
                check_near_duplicate(&haiku, &suspect).await
            }
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
                    "Review signal quality. Consider removing from story or improving evidence.".to_string(),
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
        determine if it is correctly classified. Signal types are: Event (time-bound gathering), \
        Give (available resource/service), Ask (community need requesting help), \
        Notice (official advisory/policy), Tension (community conflict or systemic problem). \
        Respond with ONLY one of: CORRECT or WRONG:<correct_type> (e.g., WRONG:Event)";

    let user = format!(
        "Signal title: {}\nSignal summary: {}\n\n{}",
        suspect.title, suspect.summary, suspect.context,
    );

    let response = claude.chat_completion(system, &user).await?;
    let response = response.trim();

    if response.starts_with("WRONG:") {
        let suggested_type = response.strip_prefix("WRONG:").unwrap_or("unknown").trim();
        Ok(Some(ValidationIssue::new(
            "",  // city set by caller
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
        Respond with COHERENT if the signals tell a unified story, or INCOHERENT followed by a brief \
        explanation of why the signals don't belong together. If incoherent, identify which signals \
        (by their title) seem out of place.";

    let user = format!(
        "Story headline: {}\nStory summary: {}\n\n{}",
        suspect.title, suspect.summary, suspect.context,
    );

    let response = claude.chat_completion(system, &user).await?;
    let response = response.trim();

    if response.starts_with("INCOHERENT") {
        let explanation = response
            .strip_prefix("INCOHERENT")
            .unwrap_or("")
            .trim_start_matches(|c: char| c == ':' || c == ' ')
            .to_string();

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
        community tension or need. Respond with VALID if the response genuinely helps address the \
        tension, or INVALID followed by a brief explanation of why the match is wrong.";

    let user = format!(
        "Signal: {} — {}\n\n{}",
        suspect.title, suspect.summary, suspect.context,
    );

    let response = claude.chat_completion(system, &user).await?;
    let response = response.trim();

    if response.starts_with("INVALID") {
        let explanation = response
            .strip_prefix("INVALID")
            .unwrap_or("")
            .trim_start_matches(|c: char| c == ':' || c == ' ')
            .to_string();

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
        They may use different words but refer to the same event, resource, need, or issue. \
        Respond with DUPLICATE if they are the same thing, or DISTINCT if they are genuinely different.";

    let user = suspect.context.clone();

    let response = claude.chat_completion(system, &user).await?;
    let response = response.trim();

    if response.starts_with("DUPLICATE") {
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
