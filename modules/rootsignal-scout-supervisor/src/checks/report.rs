use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use tracing::{info, warn};

/// Root data directory, controlled by `DATA_DIR` env var (default: `"data"`).
fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string()))
}

use super::batch_review::{BatchReviewOutput, RunAnalysis, SignalForReview, Verdict};

// =============================================================================
// Data dump
// =============================================================================

#[derive(Debug, Serialize)]
pub struct SupervisorReport {
    pub region: String,
    pub run_date: String,
    pub scout_run_id: String,
    pub signals_reviewed: u64,
    pub signals_passed: u64,
    pub signals_rejected: u64,
    pub signals: Vec<SignalForReview>,
    pub verdicts: Vec<Verdict>,
    pub run_analysis: Option<RunAnalysis>,
}

/// Save the supervisor report as JSON. Returns the file path.
pub fn save_report(region_slug: &str, output: &BatchReviewOutput) -> Result<PathBuf> {
    let scout_run_id = output
        .reviewed_signals
        .first()
        .map(|s| s.scout_run_id.as_str())
        .unwrap_or("unknown");

    let date = Utc::now().format("%Y-%m-%d").to_string();
    let dir = data_dir().join("supervisor-reports").join(region_slug);
    std::fs::create_dir_all(&dir)?;

    let path = dir.join(format!("{date}-{scout_run_id}.json"));

    let report = SupervisorReport {
        region: region_slug.to_string(),
        run_date: date,
        scout_run_id: scout_run_id.to_string(),
        signals_reviewed: output.signals_reviewed,
        signals_passed: output.signals_passed,
        signals_rejected: output.signals_rejected,
        signals: output.reviewed_signals.clone(),
        verdicts: output.verdicts.clone(),
        run_analysis: output.run_analysis.clone(),
    };

    std::fs::write(&path, serde_json::to_string_pretty(&report)?)?;
    info!(path = %path.display(), "Supervisor report saved");

    Ok(path)
}

// =============================================================================
// GitHub issue
// =============================================================================

/// Create a GitHub issue with the supervisor analysis.
/// Uses `gh issue create` CLI. Returns the issue URL on success.
pub fn create_github_issue(
    region_slug: &str,
    output: &BatchReviewOutput,
    report_path: &std::path::Path,
) -> Result<Option<String>> {
    let analysis = match &output.run_analysis {
        Some(a) => a,
        None => {
            info!("No run_analysis, skipping GitHub issue creation");
            return Ok(None);
        }
    };

    let title = format!(
        "supervisor({}): {}",
        region_slug,
        truncate(&analysis.pattern_summary, 60)
    );

    // Build rejection table
    let rejections: Vec<&Verdict> = output
        .verdicts
        .iter()
        .filter(|v| v.decision == "reject")
        .collect();

    let mut table_rows = String::new();
    for v in &rejections {
        let signal = output
            .reviewed_signals
            .iter()
            .find(|s| s.id == v.signal_id);
        let title_col = signal.map(|s| s.title.as_str()).unwrap_or("?");
        let type_col = signal.map(|s| s.signal_type.as_str()).unwrap_or("?");
        let module_col = signal.map(|s| s.created_by.as_str()).unwrap_or("?");
        let source_col = signal
            .map(|s| truncate(&s.source_url, 40))
            .unwrap_or_else(|| "?".to_string());
        let reason = v.rejection_reason.as_deref().unwrap_or("?");

        table_rows.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            truncate(title_col, 30),
            type_col,
            module_col,
            source_col,
            reason,
        ));
    }

    let body = format!(
        r#"## Supervisor Report: {region} — {date}

### Summary
{pattern}

### Analysis
- **Suspected module:** `{module}`
- **Root cause:** {root_cause}
- **Suggested fix:** {fix}

### Rejection Details ({count} signals)
| Signal | Type | Created By | Source | Reason |
|--------|------|------------|--------|--------|
{table}
### Data
Full signal data dump: `{report_path}`

### How to Investigate
1. Read the data dump to see the actual signals
2. Check `modules/rootsignal-scout/src/{module}.rs`
3. Look for the pattern described in "Root cause"
4. Run `cargo run --bin scout -- {region}` to reproduce
5. Run `cargo run --bin supervisor -- {region}` to verify fix"#,
        region = region_slug,
        date = Utc::now().format("%Y-%m-%d"),
        pattern = analysis.pattern_summary,
        module = analysis.suspected_module,
        root_cause = analysis.root_cause_hypothesis,
        fix = analysis.suggested_fix,
        count = rejections.len(),
        table = table_rows,
        report_path = report_path.display(),
    );

    // Shell out to gh CLI
    match std::process::Command::new("gh")
        .args(["issue", "create", "--title", &title, "--body", &body, "--label", "supervisor"])
        .output()
    {
        Ok(result) => {
            if result.status.success() {
                let url = String::from_utf8_lossy(&result.stdout).trim().to_string();
                info!(url = url.as_str(), "GitHub issue created");
                Ok(Some(url))
            } else {
                let stderr = String::from_utf8_lossy(&result.stderr);
                warn!(stderr = stderr.as_ref(), "gh issue create failed");
                Ok(None)
            }
        }
        Err(e) => {
            warn!(error = %e, "gh CLI not available, skipping issue creation");
            Ok(None)
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
