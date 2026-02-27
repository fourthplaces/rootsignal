use std::collections::HashMap;
use std::sync::Arc;

use ai_client::claude::Claude;
use ai_client::traits::{Agent, PromptBuilder};
use anyhow::Result;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::ScoutScope;
use rootsignal_graph::StagedSignal;

use crate::infra::run_log::{EventKind, EventLogger};
use crate::lint::lint_tools::{
    CorrectSignalTool, LintState, LintVerdict, PassSignalTool, RejectSignalTool,
    ReadSourceTool,
};
use crate::traits::{ContentFetcher, SignalStore};

const MAX_TOOL_TURNS: usize = 30;
const MAX_SIGNALS_PER_BATCH: usize = 15;

const LINT_SYSTEM_PROMPT: &str = r#"You are a signal quality auditor. Your job is to verify that extracted signals accurately represent the source content they came from.

For each batch of signals, you will:
1. First call read_source to fetch the archived content for the source URL
2. Compare every field of every signal against the source content
3. For each signal, either:
   - Call pass_signal if all fields are accurate
   - Call correct_signal for each field that needs fixing (the signal is auto-promoted after corrections)
   - Call reject_signal if the signal is fundamentally wrong (hallucinated, wrong type, no basis in source)

Rules:
- Every signal MUST receive a verdict (pass, correct+pass, or reject). Do not skip any.
- If read_source fails, reject ALL signals from that source with reason "SOURCE_UNREADABLE"
- Only correct fields that are factually wrong based on the source content. Do not make stylistic changes.
- Dates must match what the source says. If the source doesn't mention a date, the signal shouldn't have one.
- Locations must match the source. Verify coordinates are reasonable for the named location.
- Titles and summaries must reflect what the source actually says, not hallucinated content.
- action_url must be a real URL found in or derivable from the source.
- signal_type cannot be corrected — reject if the type is wrong.
"#;

pub struct SignalLinter<L: EventLogger> {
    store: Arc<dyn SignalStore>,
    fetcher: Arc<dyn ContentFetcher>,
    anthropic_api_key: String,
    region: ScoutScope,
    logger: L,
}

pub struct LintResult {
    pub passed: u32,
    pub corrected: u32,
    pub rejected: u32,
    pub situations_promoted: u32,
    pub stories_promoted: u32,
}

impl<L: EventLogger> SignalLinter<L> {
    pub fn new(
        store: Arc<dyn SignalStore>,
        fetcher: Arc<dyn ContentFetcher>,
        anthropic_api_key: String,
        region: ScoutScope,
        logger: L,
    ) -> Self {
        Self { store, fetcher, anthropic_api_key, region, logger }
    }

    pub async fn run(&self) -> Result<LintResult> {
        let (min_lat, max_lat, min_lng, max_lng) = self.region.bounding_box();

        let signals = self.store
            .staged_signals_in_region(min_lat, max_lat, min_lng, max_lng)
            .await?;

        if signals.is_empty() {
            info!(region = %self.region.name, "No staged signals to lint");
            return Ok(LintResult {
                passed: 0, corrected: 0, rejected: 0,
                situations_promoted: 0, stories_promoted: 0,
            });
        }

        info!(region = %self.region.name, count = signals.len(), "Linting staged signals");

        // Group by source_url
        let mut by_source: HashMap<String, Vec<&StagedSignal>> = HashMap::new();
        for signal in &signals {
            by_source.entry(signal.source_url.clone())
                .or_default()
                .push(signal);
        }

        let mut total_passed = 0u32;
        let mut total_corrected = 0u32;
        let mut total_rejected = 0u32;

        for (source_url, source_signals) in &by_source {
            // Split into sub-batches if needed
            for batch in source_signals.chunks(MAX_SIGNALS_PER_BATCH) {
                let (passed, corrected, rejected) = self
                    .lint_batch(source_url, batch)
                    .await?;

                total_passed += passed;
                total_corrected += corrected;
                total_rejected += rejected;
            }
        }

        // Promote situations and stories whose signals are all live
        let situations_promoted = self.store.promote_ready_situations().await?;
        let stories_promoted = self.store.promote_ready_stories().await?;

        info!(
            region = %self.region.name,
            passed = total_passed,
            corrected = total_corrected,
            rejected = total_rejected,
            situations_promoted,
            stories_promoted,
            "Signal lint complete"
        );

        Ok(LintResult {
            passed: total_passed,
            corrected: total_corrected,
            rejected: total_rejected,
            situations_promoted,
            stories_promoted,
        })
    }

    async fn lint_batch(
        &self,
        source_url: &str,
        signals: &[&StagedSignal],
    ) -> Result<(u32, u32, u32)> {
        let state = LintState::new();

        let claude = Claude::new(&self.anthropic_api_key, "claude-sonnet-4-5-20250514")
            .tool(ReadSourceTool { fetcher: self.fetcher.clone() })
            .tool(CorrectSignalTool { state: state.clone() })
            .tool(RejectSignalTool { state: state.clone() })
            .tool(PassSignalTool { state: state.clone() });

        let user_prompt = format_batch_prompt(source_url, signals);

        let result = claude
            .prompt(&user_prompt)
            .preamble(LINT_SYSTEM_PROMPT)
            .temperature(0.0)
            .multi_turn(MAX_TOOL_TURNS)
            .send()
            .await;

        if let Err(ref e) = result {
            warn!(source_url, error = %e, "Lint LLM call failed, rejecting batch");
            // On LLM failure, reject all signals in this batch
            for signal in signals {
                let id = Uuid::parse_str(&signal.id)?;
                let reason = format!("LINT_ERROR: {e}");
                self.store.set_review_status(id, "rejected", Some(&reason)).await?;
                self.logger.log(EventKind::LintRejection {
                    node_id: signal.id.clone(),
                    signal_type: signal.signal_type.clone(),
                    title: signal.title.clone(),
                    reason,
                });
            }
            let count = signals.len() as u32;
            self.logger.log(EventKind::LintBatch {
                source_url: source_url.to_string(),
                signal_count: count,
                passed: 0,
                corrected: 0,
                rejected: count,
            });
            return Ok((0, 0, count));
        }

        // Apply verdicts from tool state
        let verdicts = state.into_verdicts();
        let mut passed = 0u32;
        let mut corrected = 0u32;
        let mut rejected = 0u32;

        for signal in signals {
            let id = Uuid::parse_str(&signal.id)?;

            match verdicts.get(&signal.id) {
                Some(LintVerdict::Pass) => {
                    self.store.set_review_status(id, "live", None).await?;
                    passed += 1;
                }
                Some(LintVerdict::Correct { corrections }) => {
                    self.store.update_signal_fields(id, corrections).await?;
                    self.store.set_signal_corrected(id, corrections).await?;
                    self.store.set_review_status(id, "live", None).await?;

                    for c in corrections {
                        self.logger.log(EventKind::LintCorrection {
                            node_id: signal.id.clone(),
                            signal_type: signal.signal_type.clone(),
                            title: signal.title.clone(),
                            field: c.field.clone(),
                            old_value: c.old_value.clone(),
                            new_value: c.new_value.clone(),
                            reason: c.reason.clone(),
                        });
                    }
                    corrected += 1;
                }
                Some(LintVerdict::Reject { reason }) => {
                    self.store.set_review_status(id, "rejected", Some(reason)).await?;
                    self.logger.log(EventKind::LintRejection {
                        node_id: signal.id.clone(),
                        signal_type: signal.signal_type.clone(),
                        title: signal.title.clone(),
                        reason: reason.clone(),
                    });
                    rejected += 1;
                }
                None => {
                    // LLM skipped this signal — reject as safety net
                    let reason = "NO_VERDICT: LLM did not issue a verdict for this signal";
                    self.store.set_review_status(id, "rejected", Some(reason)).await?;
                    self.logger.log(EventKind::LintRejection {
                        node_id: signal.id.clone(),
                        signal_type: signal.signal_type.clone(),
                        title: signal.title.clone(),
                        reason: reason.to_string(),
                    });
                    rejected += 1;
                }
            }
        }

        self.logger.log(EventKind::LintBatch {
            source_url: source_url.to_string(),
            signal_count: signals.len() as u32,
            passed,
            corrected,
            rejected,
        });

        Ok((passed, corrected, rejected))
    }
}

fn format_batch_prompt(source_url: &str, signals: &[&StagedSignal]) -> String {
    let mut prompt = format!(
        "Source URL: {source_url}\n\n\
         Verify the following {} signal(s) against the source content.\n\
         First call read_source to fetch the content, then check each signal.\n\n",
        signals.len()
    );

    for (i, signal) in signals.iter().enumerate() {
        prompt.push_str(&format!("--- Signal {} ---\n", i + 1));
        prompt.push_str(&format!("node_id: {}\n", signal.id));
        prompt.push_str(&format!("signal_type: {}\n", signal.signal_type));
        prompt.push_str(&format!("title: {}\n", signal.title));
        prompt.push_str(&format!("summary: {}\n", signal.summary));
        prompt.push_str(&format!("confidence: {}\n", signal.confidence));

        if let Some(ref v) = signal.starts_at { prompt.push_str(&format!("starts_at: {v}\n")); }
        if let Some(ref v) = signal.ends_at { prompt.push_str(&format!("ends_at: {v}\n")); }
        if let Some(ref v) = signal.published_at { prompt.push_str(&format!("published_at: {v}\n")); }
        if let Some(ref v) = signal.location_name { prompt.push_str(&format!("location_name: {v}\n")); }
        if let (Some(lat), Some(lng)) = (signal.lat, signal.lng) {
            prompt.push_str(&format!("lat: {lat}\nlng: {lng}\n"));
        }
        if let Some(ref v) = signal.action_url { prompt.push_str(&format!("action_url: {v}\n")); }
        if let Some(ref v) = signal.organizer { prompt.push_str(&format!("organizer: {v}\n")); }
        if let Some(ref v) = signal.sensitivity { prompt.push_str(&format!("sensitivity: {v}\n")); }
        if let Some(ref v) = signal.severity { prompt.push_str(&format!("severity: {v}\n")); }
        if let Some(ref v) = signal.category { prompt.push_str(&format!("category: {v}\n")); }
        if let Some(ref v) = signal.what_needed { prompt.push_str(&format!("what_needed: {v}\n")); }
        if let Some(v) = signal.is_recurring { prompt.push_str(&format!("is_recurring: {v}\n")); }

        prompt.push('\n');
    }

    prompt
}
