use anyhow::Result;
use pgvector::Vector;
use rootsignal_core::ServerDeps;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use ai_client::traits::{Agent, PromptBuilder};

use crate::findings::models::connection::Connection;
use crate::findings::models::finding::Finding;
use crate::findings::models::finding_evidence::FindingEvidence;
use crate::findings::tools::*;
use crate::investigations::Investigation;
use crate::search::Embedding;

/// Structured output from the investigation agent.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct InvestigationOutput {
    title: String,
    summary: String,
    evidence: Vec<EvidenceItem>,
    connections: Vec<ConnectionItem>,
    parent_finding_id: Option<String>,
    parent_connection_role: Option<String>,
    parent_causal_quote: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct EvidenceItem {
    evidence_type: String,
    quote: String,
    attribution: Option<String>,
    url: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ConnectionItem {
    from_type: String,
    from_id: String,
    role: String,
    causal_quote: Option<String>,
    confidence: Option<f32>,
}

/// What triggered the investigation.
pub enum InvestigationTrigger {
    /// A single signal flagged during extraction.
    FlaggedSignal { signal_id: Uuid },
    /// A cluster of signals detected by batch scan.
    ClusterDetection {
        signal_ids: Vec<Uuid>,
        city: String,
    },
}

impl InvestigationTrigger {
    fn trigger_label(&self) -> String {
        match self {
            Self::FlaggedSignal { signal_id } => format!("flagged_signal:{}", signal_id),
            Self::ClusterDetection { city, .. } => format!("cluster_detection:{}", city),
        }
    }

    fn primary_signal_id(&self) -> Uuid {
        match self {
            Self::FlaggedSignal { signal_id } => *signal_id,
            Self::ClusterDetection { signal_ids, .. } => signal_ids[0],
        }
    }
}

/// Run a "why" investigation on a flagged signal or cluster.
pub async fn run_why_investigation(
    trigger: InvestigationTrigger,
    deps: &Arc<ServerDeps>,
) -> Result<Option<Finding>> {
    let pool = deps.pool();
    let primary_signal_id = trigger.primary_signal_id();

    // 1. Claim the signal (atomic — if another worker got it, bail)
    let rows_affected = sqlx::query(
        "UPDATE signals SET investigation_status = 'in_progress' WHERE id = $1 AND investigation_status = 'pending'",
    )
    .bind(primary_signal_id)
    .execute(pool)
    .await?
    .rows_affected();

    if rows_affected == 0 {
        info!(signal_id = %primary_signal_id, "Signal already claimed by another worker");
        return Ok(None);
    }

    // Load the trigger signal for context
    let signal = crate::signals::Signal::find_by_id(primary_signal_id, pool).await?;

    // 2. Check for existing Finding match via embedding similarity
    let embed_text = format!("{} {}", signal.content, signal.about.as_deref().unwrap_or(""));
    if let Ok(raw_embedding) = deps.embedding_service.embed(&embed_text).await {
        let query_vec = Vector::from(raw_embedding);
        let similar = Embedding::search_similar(query_vec, "finding", 1, 0.15, pool).await?;
        if let Some(match_record) = similar.first() {
            // Existing Finding covers this signal — link and return
            Connection::create(
                "signal",
                primary_signal_id,
                "finding",
                match_record.embeddable_id,
                "evidence_of",
                Some(&signal.content),
                Some(0.7),
                pool,
            )
            .await?;
            sqlx::query("UPDATE signals SET investigation_status = 'linked' WHERE id = $1")
                .bind(primary_signal_id)
                .execute(pool)
                .await?;
            info!(
                signal_id = %primary_signal_id,
                finding_id = %match_record.embeddable_id,
                "Linked signal to existing finding"
            );
            return Ok(None);
        }
    }

    // 3. Create investigation record
    let investigation =
        Investigation::create("signal", primary_signal_id, &trigger.trigger_label(), pool).await?;
    Investigation::update_status(investigation.id, "running", pool).await?;

    info!(
        investigation_id = %investigation.id,
        signal_id = %primary_signal_id,
        "Starting why-investigation"
    );

    // 4. Build the agent with investigation tools
    let agent = (*deps.ai)
        .clone()
        .tool(FollowLinkTool::new(
            deps.ingestor.clone(),
            pool.clone(),
            investigation.id,
        ))
        .tool(FindingWebSearchTool::new(
            deps.web_searcher.clone(),
            pool.clone(),
            investigation.id,
        ))
        .tool(QuerySignalsTool::new(pool.clone(), investigation.id))
        .tool(QuerySocialTool::new(pool.clone(), investigation.id))
        .tool(QueryEntitiesTool::new(pool.clone(), investigation.id))
        .tool(QueryFindingsTool::new(pool.clone(), investigation.id))
        .tool(RecommendSourceTool::new(pool.clone(), investigation.id));

    // 5. Build the investigation prompt
    let mut context_parts = vec![format!(
        "Investigate this flagged signal:\n\nType: {}\nContent: {}\nAbout: {}",
        signal.signal_type,
        signal.content,
        signal.about.as_deref().unwrap_or("(not specified)")
    )];

    if let Some(ref reason) = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT investigation_reason FROM signals WHERE id = $1",
    )
    .bind(primary_signal_id)
    .fetch_one(pool)
    .await?
    .0
    {
        context_parts.push(format!("Investigation reason: {}", reason));
    }

    if let Some(ref source_url) = signal.source_url {
        context_parts.push(format!("Source URL: {}", source_url));
    }

    context_parts.push(format!(
        "\nThe trigger signal ID is: {}\nUse this ID in your connections output.",
        primary_signal_id
    ));

    // For cluster triggers, add context about the cluster
    if let InvestigationTrigger::ClusterDetection {
        ref signal_ids,
        ref city,
    } = trigger
    {
        context_parts.push(format!(
            "\nThis was triggered by a cluster of {} signals in {}.",
            signal_ids.len(),
            city
        ));
    }

    let user_prompt = context_parts.join("\n");
    let system_prompt = deps.prompts.finding_investigation_prompt();

    // 6. Run the multi-turn agent
    let response = agent
        .prompt(&user_prompt)
        .preamble(system_prompt)
        .multi_turn(10)
        .send()
        .await;

    match response {
        Ok(text) => {
            // 7. Parse structured output
            let parsed = parse_investigation_output(&text);

            match parsed {
                Some(output) => {
                    // 8. Run adversarial validation (Phase 4)
                    let validation = super::validate::validate_finding(&output, &text, deps).await;
                    let rejected = validation
                        .as_ref()
                        .map(|v| v.rejected)
                        .unwrap_or(false);

                    if rejected {
                        let reasoning = validation
                            .as_ref()
                            .map(|v| v.reasoning.as_str())
                            .unwrap_or("validation rejected");
                        Investigation::complete(
                            investigation.id,
                            &format!("Rejected: {}", reasoning),
                            0.0,
                            pool,
                        )
                        .await?;
                        sqlx::query(
                            "UPDATE signals SET investigation_status = 'completed' WHERE id = $1",
                        )
                        .bind(primary_signal_id)
                        .execute(pool)
                        .await?;
                        info!(
                            investigation_id = %investigation.id,
                            "Investigation rejected by validation"
                        );
                        return Ok(None);
                    }

                    // 9. Dedup check before insert
                    let finding_embed_text =
                        format!("{} {}", output.title, output.summary);
                    let mut dedup_finding_id = None;
                    if let Ok(raw_emb) = deps.embedding_service.embed(&finding_embed_text).await {
                        let query_vec = Vector::from(raw_emb);
                        let similar =
                            Embedding::search_similar(query_vec, "finding", 1, 0.1, pool).await?;
                        if let Some(dup) = similar.first() {
                            dedup_finding_id = Some(dup.embeddable_id);
                        }
                    }

                    if let Some(existing_id) = dedup_finding_id {
                        // Near-duplicate — link instead of creating
                        Connection::create(
                            "signal",
                            primary_signal_id,
                            "finding",
                            existing_id,
                            "evidence_of",
                            Some(&signal.content),
                            Some(0.7),
                            pool,
                        )
                        .await?;
                        Investigation::complete(
                            investigation.id,
                            &format!("Linked to existing finding {}", existing_id),
                            0.7,
                            pool,
                        )
                        .await?;
                        sqlx::query(
                            "UPDATE signals SET investigation_status = 'linked' WHERE id = $1",
                        )
                        .bind(primary_signal_id)
                        .execute(pool)
                        .await?;
                        return Ok(Some(Finding::find_by_id(existing_id, pool).await?));
                    }

                    // 10. Create Finding + evidence + connections + embedding
                    let mut fingerprint_hasher = Sha256::new();
                    fingerprint_hasher.update(output.title.as_bytes());
                    fingerprint_hasher.update(output.summary.as_bytes());
                    let fingerprint = fingerprint_hasher.finalize().to_vec();

                    let finding = Finding::create(
                        &output.title,
                        &output.summary,
                        &fingerprint,
                        Some(investigation.id),
                        Some(primary_signal_id),
                        pool,
                    )
                    .await?;

                    // Set validation status
                    Finding::update_validation_status(finding.id, "validated", pool).await?;

                    // Create evidence records
                    for ev in &output.evidence {
                        FindingEvidence::create(
                            finding.id,
                            &ev.evidence_type,
                            &ev.quote,
                            ev.attribution.as_deref(),
                            ev.url.as_deref(),
                            None,
                            pool,
                        )
                        .await?;
                    }

                    // Create connections
                    for conn in &output.connections {
                        if let Ok(from_id) = conn.from_id.parse::<Uuid>() {
                            Connection::create(
                                &conn.from_type,
                                from_id,
                                "finding",
                                finding.id,
                                &conn.role,
                                conn.causal_quote.as_deref(),
                                conn.confidence,
                                pool,
                            )
                            .await?;
                        }
                    }

                    // Create parent finding connection if specified
                    if let Some(ref parent_id_str) = output.parent_finding_id {
                        if let Ok(parent_id) = parent_id_str.parse::<Uuid>() {
                            let role = output
                                .parent_connection_role
                                .as_deref()
                                .unwrap_or("driven_by");
                            Connection::create(
                                "finding",
                                finding.id,
                                "finding",
                                parent_id,
                                role,
                                output.parent_causal_quote.as_deref(),
                                Some(0.7),
                                pool,
                            )
                            .await?;
                        }
                    }

                    // Create embedding for the finding
                    if let Ok(raw_emb) = deps.embedding_service.embed(&finding_embed_text).await {
                        let vector = Vector::from(raw_emb);
                        let mut hash_hasher = Sha256::new();
                        hash_hasher.update(finding_embed_text.as_bytes());
                        let hash = hex::encode(hash_hasher.finalize());
                        let _ = Embedding::upsert(
                            "finding",
                            finding.id,
                            "en",
                            vector,
                            &hash,
                            pool,
                        )
                        .await;
                    }

                    // 11. Sweep related pending signals
                    sweep_related_signals(finding.id, &finding_embed_text, deps).await?;

                    // 12. Process source recommendations
                    process_source_recommendations(investigation.id, deps).await?;

                    // Mark investigation complete
                    Investigation::complete(investigation.id, &output.summary, 0.8, pool).await?;

                    // Mark original signal completed
                    sqlx::query(
                        "UPDATE signals SET investigation_status = 'completed' WHERE id = $1",
                    )
                    .bind(primary_signal_id)
                    .execute(pool)
                    .await?;

                    info!(
                        investigation_id = %investigation.id,
                        finding_id = %finding.id,
                        "Finding created"
                    );

                    Ok(Some(finding))
                }
                None => {
                    // Could not parse structured output
                    Investigation::complete(
                        investigation.id,
                        &format!("Unstructured response: {}", &text[..text.len().min(500)]),
                        0.3,
                        pool,
                    )
                    .await?;
                    sqlx::query(
                        "UPDATE signals SET investigation_status = 'completed' WHERE id = $1",
                    )
                    .bind(primary_signal_id)
                    .execute(pool)
                    .await?;
                    Ok(None)
                }
            }
        }
        Err(e) => {
            let error_msg = format!("Investigation failed: {}", e);
            Investigation::update_status(investigation.id, "failed", pool).await?;
            sqlx::query("UPDATE signals SET investigation_status = 'pending' WHERE id = $1")
                .bind(primary_signal_id)
                .execute(pool)
                .await?;
            Err(anyhow::anyhow!(error_msg))
        }
    }
}

/// Parse the agent's response, extracting JSON from the text.
fn parse_investigation_output(text: &str) -> Option<InvestigationOutput> {
    // Try to find JSON in the response (between ```json and ``` or raw JSON object)
    let json_str = if let Some(start) = text.find("```json") {
        let start = start + 7;
        let end = text[start..].find("```").map(|e| start + e)?;
        &text[start..end]
    } else if let Some(start) = text.find('{') {
        // Find the matching closing brace
        let mut depth = 0;
        let mut end = start;
        for (i, ch) in text[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        &text[start..end]
    } else {
        return None;
    };

    serde_json::from_str(json_str.trim()).ok()
}

/// Find pending signals near the new finding and link them.
async fn sweep_related_signals(
    finding_id: Uuid,
    finding_text: &str,
    deps: &Arc<ServerDeps>,
) -> Result<()> {
    let pool = deps.pool();

    let raw_emb = match deps.embedding_service.embed(finding_text).await {
        Ok(emb) => emb,
        Err(_) => return Ok(()),
    };
    let query_vec = Vector::from(raw_emb);

    // Find pending signals with similar embeddings
    let similar = Embedding::search_similar(query_vec, "signal", 20, 0.2, pool).await?;

    for record in similar {
        // Check if this signal is pending investigation
        let row = sqlx::query_as::<_, (Option<bool>, Option<String>)>(
            "SELECT needs_investigation, investigation_status FROM signals WHERE id = $1",
        )
        .bind(record.embeddable_id)
        .fetch_optional(pool)
        .await?;

        if let Some((Some(true), Some(status))) = row {
            if status == "pending" {
                Connection::create(
                    "signal",
                    record.embeddable_id,
                    "finding",
                    finding_id,
                    "evidence_of",
                    None,
                    Some(1.0 - record.distance as f32),
                    pool,
                )
                .await?;
                sqlx::query(
                    "UPDATE signals SET investigation_status = 'linked' WHERE id = $1",
                )
                .bind(record.embeddable_id)
                .execute(pool)
                .await?;
                info!(
                    signal_id = %record.embeddable_id,
                    finding_id = %finding_id,
                    "Swept related signal"
                );
            }
        }
    }

    Ok(())
}

/// Process source recommendations recorded during investigation.
async fn process_source_recommendations(
    investigation_id: Uuid,
    deps: &Arc<ServerDeps>,
) -> Result<()> {
    let pool = deps.pool();

    let steps = crate::findings::InvestigationStep::find_by_investigation(investigation_id, pool)
        .await?;

    for step in steps {
        if step.tool_name == "recommend_source" {
            if let Some(url) = step.input.get("url").and_then(|v| v.as_str()) {
                // Create source if it doesn't already exist
                let exists = sqlx::query_as::<_, (i64,)>(
                    "SELECT COUNT(*) FROM sources WHERE url = $1",
                )
                .bind(url)
                .fetch_one(pool)
                .await?;

                if exists.0 == 0 {
                    let reason = step
                        .input
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Recommended by investigation agent");

                    sqlx::query(
                        "INSERT INTO sources (url, content_summary) VALUES ($1, $2) ON CONFLICT (url) DO NOTHING",
                    )
                    .bind(url)
                    .bind(reason)
                    .execute(pool)
                    .await?;

                    info!(url = url, "Created recommended source");
                }
            }
        }
    }

    Ok(())
}
