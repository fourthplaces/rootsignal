use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::info;
use uuid::Uuid;

use rootsignal_graph::GraphClient;

/// A suspect identified by cheap heuristic triage, pending LLM review.
#[derive(Debug, Clone)]
pub struct Suspect {
    pub id: Uuid,
    pub label: String,
    pub title: String,
    pub summary: String,
    pub check_type: SuspectType,
    /// Extra context for the LLM check (e.g., evidence snippets, paired signal).
    pub context: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspectType {
    Misclassification,
    IncoherentStory,
    BadRespondsTo,
    NearDuplicate,
    LowConfidenceHighVisibility,
}

/// Run all triage heuristics and return a list of suspects for LLM review.
pub async fn triage_suspects(
    client: &GraphClient,
    from: &DateTime<Utc>,
    to: &DateTime<Utc>,
) -> Result<Vec<Suspect>, neo4rs::Error> {
    let mut suspects = Vec::new();

    let from_ts = rootsignal_graph::writer::format_datetime_pub(from);
    let to_ts = rootsignal_graph::writer::format_datetime_pub(to);

    suspects.extend(triage_misclassification(client, &from_ts, &to_ts).await?);
    suspects.extend(triage_incoherent_stories(client, &from_ts, &to_ts).await?);
    suspects.extend(triage_bad_responds_to(client, &from_ts, &to_ts).await?);
    suspects.extend(triage_near_duplicates(client, &from_ts, &to_ts).await?);
    suspects.extend(triage_low_confidence_high_visibility(client, &from_ts, &to_ts).await?);

    info!(
        total = suspects.len(),
        "Triage complete"
    );

    Ok(suspects)
}

/// Signals with low confidence AND only a single evidence source.
/// These are most likely to be misclassified by the LLM.
async fn triage_misclassification(
    client: &GraphClient,
    from_ts: &str,
    to_ts: &str,
) -> Result<Vec<Suspect>, neo4rs::Error> {
    let mut suspects = Vec::new();

    for label in &["Event", "Give", "Ask", "Notice", "Tension"] {
        let q = query(&format!(
            "MATCH (n:{label})
             WHERE n.extracted_at >= datetime($from) AND n.extracted_at <= datetime($to)
               AND n.confidence < 0.5
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             WITH n, count(ev) AS ev_count, collect(ev.snippet) AS snippets
             WHERE ev_count <= 1
             RETURN n.id AS id, n.title AS title, n.summary AS summary,
                    snippets"
        ))
        .param("from", from_ts.to_string())
        .param("to", to_ts.to_string());

        let mut stream = client.inner().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let snippets: Vec<String> = row.get("snippets").unwrap_or_default();
            let context = format!(
                "Current type: {label}\nEvidence snippets:\n{}",
                snippets.join("\n---\n")
            );

            suspects.push(Suspect {
                id,
                label: label.to_string(),
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                check_type: SuspectType::Misclassification,
                context,
            });
        }
    }

    if !suspects.is_empty() {
        info!(count = suspects.len(), "Misclassification suspects found");
    }
    Ok(suspects)
}

/// Stories whose constituent signals have few shared actors and many different types.
/// Suggests the clustering grouped semantically similar but narratively unrelated signals.
async fn triage_incoherent_stories(
    client: &GraphClient,
    from_ts: &str,
    to_ts: &str,
) -> Result<Vec<Suspect>, neo4rs::Error> {
    let q = query(
        "MATCH (s:Story)-[:CONTAINS]->(sig)
         WHERE s.last_updated >= datetime($from) AND s.last_updated <= datetime($to)
         WITH s, collect(sig) AS signals,
              count(DISTINCT labels(sig)[0]) AS type_count
         WHERE type_count >= 3 AND size(signals) >= 3
         OPTIONAL MATCH (sig2)-[:ACTED_IN]->(a:Actor)
         WHERE sig2 IN signals
         WITH s, signals, type_count,
              count(DISTINCT a.id) AS actor_count,
              count(sig2) AS signals_with_actors
         WHERE actor_count < 2 OR (signals_with_actors * 1.0 / size(signals)) < 0.3
         RETURN s.id AS id, s.headline AS title, s.summary AS summary,
                type_count, actor_count, size(signals) AS signal_count"
    )
    .param("from", from_ts.to_string())
    .param("to", to_ts.to_string());

    let mut suspects = Vec::new();
    let mut stream = client.inner().execute(q).await?;
    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("id").unwrap_or_default();
        let id = match Uuid::parse_str(&id_str) {
            Ok(id) => id,
            Err(_) => continue,
        };
        let type_count: i64 = row.get("type_count").unwrap_or(0);
        let actor_count: i64 = row.get("actor_count").unwrap_or(0);
        let signal_count: i64 = row.get("signal_count").unwrap_or(0);

        // Fetch signal titles for LLM context
        let signals_q = query(
            "MATCH (s:Story {id: $id})-[:CONTAINS]->(sig)
             RETURN labels(sig)[0] AS label, sig.title AS title, sig.summary AS summary
             LIMIT 20"
        )
        .param("id", id_str.clone());

        let mut sig_stream = client.inner().execute(signals_q).await?;
        let mut signal_lines = Vec::new();
        while let Some(sig_row) = sig_stream.next().await? {
            let sig_label: String = sig_row.get("label").unwrap_or_default();
            let sig_title: String = sig_row.get("title").unwrap_or_default();
            signal_lines.push(format!("[{sig_label}] {sig_title}"));
        }

        let context = format!(
            "Signal count: {signal_count}, Type diversity: {type_count}, Shared actors: {actor_count}\n\
             Signals:\n{}",
            signal_lines.join("\n")
        );

        suspects.push(Suspect {
            id,
            label: "Story".to_string(),
            title: row.get("title").unwrap_or_default(),
            summary: row.get("summary").unwrap_or_default(),
            check_type: SuspectType::IncoherentStory,
            context,
        });
    }

    if !suspects.is_empty() {
        info!(count = suspects.len(), "Incoherent story suspects found");
    }
    Ok(suspects)
}

/// RESPONDS_TO edges where the Give/Event has low confidence or the Tension is in a different story.
async fn triage_bad_responds_to(
    client: &GraphClient,
    from_ts: &str,
    to_ts: &str,
) -> Result<Vec<Suspect>, neo4rs::Error> {
    let q = query(
        "MATCH (responder)-[r:RESPONDS_TO]->(tension)
         WHERE responder.extracted_at >= datetime($from)
           AND responder.extracted_at <= datetime($to)
           AND responder.confidence < 0.4
         RETURN responder.id AS responder_id,
                labels(responder)[0] AS responder_label,
                responder.title AS responder_title,
                responder.summary AS responder_summary,
                tension.id AS tension_id,
                tension.title AS tension_title,
                tension.summary AS tension_summary,
                r.match_strength AS match_strength,
                r.explanation AS explanation"
    )
    .param("from", from_ts.to_string())
    .param("to", to_ts.to_string());

    let mut suspects = Vec::new();
    let mut stream = client.inner().execute(q).await?;
    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("responder_id").unwrap_or_default();
        let id = match Uuid::parse_str(&id_str) {
            Ok(id) => id,
            Err(_) => continue,
        };

        let tension_title: String = row.get("tension_title").unwrap_or_default();
        let tension_summary: String = row.get("tension_summary").unwrap_or_default();
        let explanation: String = row.get("explanation").unwrap_or_default();
        let match_strength: f64 = row.get("match_strength").unwrap_or(0.0);

        let context = format!(
            "Responder: {} (confidence < 0.4)\n\
             Tension: {}\nTension summary: {}\n\
             Match strength: {:.2}\nOriginal explanation: {}",
            row.get::<String>("responder_title").unwrap_or_default(),
            tension_title,
            tension_summary,
            match_strength,
            explanation,
        );

        suspects.push(Suspect {
            id,
            label: row.get("responder_label").unwrap_or_default(),
            title: row.get("responder_title").unwrap_or_default(),
            summary: row.get("responder_summary").unwrap_or_default(),
            check_type: SuspectType::BadRespondsTo,
            context,
        });
    }

    if !suspects.is_empty() {
        info!(count = suspects.len(), "Bad RESPONDS_TO suspects found");
    }
    Ok(suspects)
}

/// Signals in the 0.85–0.92 similarity range that may be near-duplicates.
async fn triage_near_duplicates(
    client: &GraphClient,
    from_ts: &str,
    to_ts: &str,
) -> Result<Vec<Suspect>, neo4rs::Error> {
    let q = query(
        "MATCH (a)-[r:SIMILAR_TO]-(b)
         WHERE a.extracted_at >= datetime($from) AND a.extracted_at <= datetime($to)
           AND r.weight >= 0.85 AND r.weight < 0.92
           AND a.id < b.id
         RETURN a.id AS id_a, labels(a)[0] AS label_a, a.title AS title_a, a.summary AS summary_a,
                b.id AS id_b, labels(b)[0] AS label_b, b.title AS title_b, b.summary AS summary_b,
                r.weight AS similarity"
    )
    .param("from", from_ts.to_string())
    .param("to", to_ts.to_string());

    let mut suspects = Vec::new();
    let mut stream = client.inner().execute(q).await?;
    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("id_a").unwrap_or_default();
        let id = match Uuid::parse_str(&id_str) {
            Ok(id) => id,
            Err(_) => continue,
        };
        let similarity: f64 = row.get("similarity").unwrap_or(0.0);
        let title_b: String = row.get("title_b").unwrap_or_default();
        let summary_b: String = row.get("summary_b").unwrap_or_default();

        let context = format!(
            "Similarity: {similarity:.3}\n\
             Signal A: {}\nSummary A: {}\n\
             Signal B: {title_b}\nSummary B: {summary_b}",
            row.get::<String>("title_a").unwrap_or_default(),
            row.get::<String>("summary_a").unwrap_or_default(),
        );

        suspects.push(Suspect {
            id,
            label: row.get("label_a").unwrap_or_default(),
            title: row.get("title_a").unwrap_or_default(),
            summary: row.get("summary_a").unwrap_or_default(),
            check_type: SuspectType::NearDuplicate,
            context,
        });
    }

    if !suspects.is_empty() {
        info!(count = suspects.len(), "Near-duplicate suspects found");
    }
    Ok(suspects)
}

/// Signals with very low confidence that appear in confirmed stories or editions.
async fn triage_low_confidence_high_visibility(
    client: &GraphClient,
    from_ts: &str,
    to_ts: &str,
) -> Result<Vec<Suspect>, neo4rs::Error> {
    let q = query(
        "MATCH (s:Story {status: 'confirmed'})-[:CONTAINS]->(sig)
         WHERE sig.extracted_at >= datetime($from) AND sig.extracted_at <= datetime($to)
           AND sig.confidence < 0.3
         RETURN sig.id AS id, labels(sig)[0] AS label,
                sig.title AS title, sig.summary AS summary,
                sig.confidence AS confidence,
                s.headline AS story_headline"
    )
    .param("from", from_ts.to_string())
    .param("to", to_ts.to_string());

    let mut suspects = Vec::new();
    let mut stream = client.inner().execute(q).await?;
    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("id").unwrap_or_default();
        let id = match Uuid::parse_str(&id_str) {
            Ok(id) => id,
            Err(_) => continue,
        };
        let confidence: f64 = row.get("confidence").unwrap_or(0.0);
        let story_headline: String = row.get("story_headline").unwrap_or_default();

        let context = format!(
            "Confidence: {confidence:.2}\nIn confirmed story: {story_headline}"
        );

        suspects.push(Suspect {
            id,
            label: row.get("label").unwrap_or_default(),
            title: row.get("title").unwrap_or_default(),
            summary: row.get("summary").unwrap_or_default(),
            check_type: SuspectType::LowConfidenceHighVisibility,
            context,
        });
    }

    if !suspects.is_empty() {
        info!(count = suspects.len(), "Low-confidence high-visibility suspects found");
    }
    Ok(suspects)
}
