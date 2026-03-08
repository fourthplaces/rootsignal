//! Converts WorldEvent signal data into SignalForReview structs for the
//! batch reviewer. Thin adapter — all review logic lives in batch_review.

use rootsignal_scout_supervisor::checks::batch_review::SignalForReview;
use rootsignal_common::events::WorldEvent;

/// Build a `SignalForReview` from a signal WorldEvent.
/// Returns None for non-signal world events.
pub fn signal_for_review(event: &WorldEvent, run_id: &str) -> Option<SignalForReview> {
    let id = event.signal_id()?.to_string();
    let title = event.title()?.to_string();
    let summary = event.summary()?.to_string();
    let source_url = event.url()?.to_string();
    let signal_type = event.node_type_label()?.to_string();

    Some(SignalForReview {
        id,
        signal_type,
        title,
        summary,
        confidence: 0.0,
        source_url,
        lat: 0.0,
        lng: 0.0,
        created_by: "scraper".to_string(),
        scout_run_id: run_id.to_string(),
        situation_headline: None,
        triage_flags: Vec::new(),
    })
}
