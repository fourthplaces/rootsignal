use tracing::info;

use rootsignal_common::types::{Severity, SourceNode};

use crate::writer::{FieldCorrection, GraphStore, NoticeInferenceRow};

/// A source is trusted when it has sustained history, corroborated output,
/// and hasn't been penalized for quality issues.
pub fn is_source_trusted(source: &SourceNode) -> bool {
    source.scrape_count >= 10
        && source.signals_corroborated >= 3
        && source.quality_penalty >= 0.7
        && source.avg_signals_per_scrape < 20.0
}

/// Re-evaluate a Notice's severity based on source trust, corroboration,
/// and EVIDENCE_OF linkage. Returns the (possibly elevated) severity.
///
/// - Trusted source + EVIDENCE_OF → at least High
/// - Trusted source, no EVIDENCE_OF → at least Medium
/// - Unknown source, no EVIDENCE_OF → Low (regardless of extracted)
/// - Unknown + EVIDENCE_OF + 2+ diverse sources → at least High
/// - Unknown + EVIDENCE_OF + corroboration ≥ 2 → at least Medium
/// - Otherwise → extracted as-is
pub fn infer_notice_severity(
    extracted_severity: Severity,
    source_trusted: bool,
    evidence_of_tension: bool,
    corroboration_count: u32,
    source_diversity: u32,
) -> Severity {
    if source_trusted && evidence_of_tension {
        return extracted_severity.max(Severity::High);
    }
    if source_trusted {
        return extracted_severity.max(Severity::Medium);
    }
    if !evidence_of_tension {
        return Severity::Low;
    }
    if source_diversity >= 2 {
        return extracted_severity.max(Severity::High);
    }
    if corroboration_count >= 2 {
        return extracted_severity.max(Severity::Medium);
    }
    extracted_severity
}

fn parse_severity(s: &str) -> Severity {
    match s {
        "high" => Severity::High,
        "critical" => Severity::Critical,
        "low" => Severity::Low,
        _ => Severity::Medium,
    }
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}

/// Re-evaluate severity for all Notices in a bounding box.
/// Fetches inference data in one batch query, applies pure inference logic,
/// and writes back any changes via `update_signal_fields`.
/// Returns the number of notices whose severity was updated.
pub async fn run_severity_inference(
    writer: &GraphStore,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
) -> anyhow::Result<u32> {
    let rows = writer
        .notice_inference_batch(min_lat, max_lat, min_lng, max_lng)
        .await?;

    let mut updated = 0u32;
    for row in &rows {
        let extracted = parse_severity(&row.severity);
        let trusted = row
            .source
            .as_ref()
            .map(|s| is_source_trusted(s))
            .unwrap_or(false);
        let inferred = infer_notice_severity(
            extracted,
            trusted,
            row.has_evidence_of,
            row.corroboration_count,
            row.source_diversity,
        );
        if inferred != extracted {
            let correction = FieldCorrection {
                field: "severity".to_string(),
                old_value: severity_str(extracted).to_string(),
                new_value: severity_str(inferred).to_string(),
                reason: "severity_inference".to_string(),
            };
            writer
                .update_signal_fields(row.notice_id, &[correction])
                .await?;
            updated += 1;
        }
    }

    if updated > 0 {
        info!(updated, total = rows.len(), "Severity inference complete");
    }

    Ok(updated)
}
