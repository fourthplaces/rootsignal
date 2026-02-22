use anyhow::Result;
use neo4rs::query;
use tracing::info;
use uuid::Uuid;

use rootsignal_graph::GraphClient;

/// Detect echo signatures — high signal volume with low type/entity diversity.
///
/// Computes echo_score for each situation:
///   echo_score = 1.0 - (type_diversity * entity_diversity)
/// where both are normalized to [0, 1]. A score near 1.0 means the situation
/// looks like a single-source echo chamber; near 0.0 means diverse corroboration.
///
/// Returns the number of situations flagged (echo_score > threshold).
pub async fn detect_echoes(client: &GraphClient, threshold: f64) -> Result<EchoStats> {
    let mut stats = EchoStats::default();

    // Find situations with enough signals to evaluate
    let q = query(
        "MATCH (sig)-[:EVIDENCES]->(s:Situation)
         WITH s, count(sig) AS signal_count,
              count(DISTINCT labels(sig)[0]) AS type_count
         WHERE signal_count >= 5
         OPTIONAL MATCH (sig2)-[:EVIDENCES]->(s) WHERE (sig2)-[:ACTED_IN]->(:Actor)
         WITH s, signal_count, type_count,
              count(DISTINCT sig2) AS sigs_with_actors
         OPTIONAL MATCH (sig3)-[:EVIDENCES]->(s), (sig3)-[:ACTED_IN]->(a:Actor)
         WITH s, signal_count, type_count,
              count(DISTINCT a.id) AS entity_count
         RETURN s.id AS id, s.headline AS headline,
                signal_count, type_count, entity_count",
    );

    let mut stream = client.inner().execute(q).await?;
    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("id").unwrap_or_default();
        let id = match Uuid::parse_str(&id_str) {
            Ok(id) => id,
            Err(_) => continue,
        };

        let signal_count: i64 = row.get("signal_count").unwrap_or(0);
        let type_count: i64 = row.get("type_count").unwrap_or(1);
        let entity_count: i64 = row.get("entity_count").unwrap_or(0);

        let echo_score = compute_echo_score(signal_count, type_count, entity_count);

        // Write echo_score to the situation node
        let update = query("MATCH (s:Situation {id: $id}) SET s.echo_score = $score")
            .param("id", id.to_string())
            .param("score", echo_score);

        if let Err(e) = client.inner().run(update).await {
            tracing::warn!(id = %id, error = %e, "Failed to write echo_score");
            continue;
        }

        stats.stories_scored += 1;

        if echo_score > threshold {
            stats.echoes_flagged += 1;
            info!(
                situation_id = %id,
                headline = row.get::<String>("headline").unwrap_or_default().as_str(),
                signal_count,
                type_count,
                entity_count,
                echo_score = format!("{echo_score:.2}").as_str(),
                "Echo signature detected"
            );
        }
    }

    if stats.echoes_flagged > 0 {
        info!(
            scored = stats.stories_scored,
            flagged = stats.echoes_flagged,
            "Echo detection complete"
        );
    }

    Ok(stats)
}

/// Compute echo score from signal count, type diversity, and entity diversity.
///
/// Returns 0.0 (diverse corroboration) to 1.0 (pure echo).
fn compute_echo_score(signal_count: i64, type_count: i64, entity_count: i64) -> f64 {
    if signal_count <= 1 {
        return 0.0;
    }

    // Type diversity: ratio of unique types to total signals, capped at 1.0
    // 5 signal types across 10 signals = 0.5 diversity
    let type_diversity = (type_count as f64 / signal_count as f64).min(1.0);

    // Entity diversity: ratio of unique entities to total signals, capped at 1.0
    // More entities = more diverse sourcing
    let entity_diversity = (entity_count as f64 / signal_count as f64).min(1.0);

    // Combined diversity: geometric mean gives credit to both dimensions
    let combined = (type_diversity * entity_diversity).sqrt();

    // Echo score is inverse of diversity
    (1.0 - combined).clamp(0.0, 1.0)
}

#[derive(Debug, Default)]
pub struct EchoStats {
    pub stories_scored: u64,
    pub echoes_flagged: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diverse_story_low_echo() {
        // 10 signals, 5 types, 8 entities = genuinely diverse
        let score = compute_echo_score(10, 5, 8);
        assert!(
            score < 0.4,
            "Diverse story should have low echo score, got {score}"
        );
    }

    #[test]
    fn echo_chamber_high_score() {
        // 15 signals, 1 type, 1 entity = pure echo
        let score = compute_echo_score(15, 1, 1);
        assert!(
            score > 0.7,
            "Echo chamber should have high score, got {score}"
        );
    }

    #[test]
    fn moderate_diversity() {
        // 10 signals, 2 types, 3 entities = moderate
        let score = compute_echo_score(10, 2, 3);
        assert!(
            score > 0.3 && score < 0.8,
            "Moderate diversity should have mid score, got {score}"
        );
    }

    #[test]
    fn single_signal_no_echo() {
        let score = compute_echo_score(1, 1, 1);
        assert_eq!(score, 0.0, "Single signal cannot be echo");
    }
}
