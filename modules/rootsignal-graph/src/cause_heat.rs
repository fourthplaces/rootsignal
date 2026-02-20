use neo4rs::query;
use tracing::info;

use crate::GraphClient;

/// A signal with its embedding and source diversity, loaded for batch computation.
struct SignalEmbed {
    id: String,
    label: String,
    embedding: Vec<f64>,
    source_diversity: u32,
}

/// Compute cause_heat for all signals in the graph.
///
/// Cause heat measures how much independent community attention exists in a
/// signal's semantic neighborhood. A food shelf Need near a hot housing cluster
/// gets boosted — not because it posted a lot, but because the *cause* it serves
/// has genuine multi-source attention.
///
/// Algorithm:
/// 1. Load all signals with embeddings and source_diversity
/// 2. Compute all-pairs cosine similarity in memory
/// 3. For each signal, sum (similarity × neighbor.source_diversity) for Tension
///    neighbors above threshold. Only Tensions radiate heat — Gatherings, Gives, Needs,
///    and Notices absorb heat from nearby Tensions but do not generate it.
/// 4. Normalize to 0.0–1.0
/// 5. Write back to graph
pub async fn compute_cause_heat(client: &GraphClient, threshold: f64) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!(threshold, "Computing cause heat...");

    // 1. Load all signals with embeddings
    let mut signals: Vec<SignalEmbed> = Vec::new();

    for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
        let q = query(&format!(
            "MATCH (n:{label})
             WHERE n.embedding IS NOT NULL
             RETURN n.id AS id, n.embedding AS embedding,
                    n.source_diversity AS source_diversity"
        ));

        let mut stream = g.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id: String = row.get("id").unwrap_or_default();
            let embedding: Vec<f64> = row.get("embedding").unwrap_or_default();
            let source_diversity: i64 = row.get("source_diversity").unwrap_or(1);

            if embedding.is_empty() {
                continue;
            }

            signals.push(SignalEmbed {
                id,
                label: label.to_string(),
                embedding,
                source_diversity: source_diversity.max(1) as u32,
            });
        }
    }

    let n = signals.len();
    info!(signals = n, "Loaded signal embeddings");

    if n == 0 {
        return Ok(());
    }

    let heats = compute_heats(&signals, threshold);

    // 5. Write back
    let mut updated = 0u32;
    for (i, signal) in signals.iter().enumerate() {
        let q = query(&format!(
            "MATCH (n:{} {{id: $id}}) SET n.cause_heat = $heat",
            signal.label
        ))
        .param("id", signal.id.as_str())
        .param("heat", heats[i]);

        g.run(q).await?;
        updated += 1;
    }

    let max_heat = heats.iter().cloned().fold(0.0_f64, f64::max);
    info!(updated, max_heat, "Cause heat computation complete");
    Ok(())
}

/// Pure computation of cause heat scores from signal embeddings.
/// Returns normalized 0.0–1.0 heat scores, one per signal.
fn compute_heats(signals: &[SignalEmbed], threshold: f64) -> Vec<f64> {
    let n = signals.len();
    if n == 0 {
        return Vec::new();
    }

    // Precompute norms
    let norms: Vec<f64> = signals
        .iter()
        .map(|s| s.embedding.iter().map(|x| x * x).sum::<f64>().sqrt())
        .collect();

    // Compute raw cause_heat for each signal
    let mut heats: Vec<f64> = vec![0.0; n];

    for i in 0..n {
        let mut heat = 0.0;
        for j in 0..n {
            if i == j {
                continue;
            }
            // Only Tensions radiate heat. A signal's cause_heat reflects how
            // well the system understands its causal tension — not how many
            // similar signals exist nearby.
            if signals[j].label != "Tension" {
                continue;
            }
            let sim = cosine_similarity(
                &signals[i].embedding,
                &signals[j].embedding,
                norms[i],
                norms[j],
            );
            if sim > threshold {
                heat += sim * signals[j].source_diversity as f64;
            }
        }
        heats[i] = heat;
    }

    // Normalize to 0.0–1.0
    let max_heat = heats.iter().cloned().fold(0.0_f64, f64::max);
    if max_heat > 0.0 {
        for h in &mut heats {
            *h /= max_heat;
        }
    }

    heats
}

/// Cosine similarity with precomputed norms.
fn cosine_similarity(a: &[f64], b: &[f64], norm_a: f64, norm_b: f64) -> f64 {
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(v: &[f64]) -> f64 {
        v.iter().map(|x| x * x).sum::<f64>().sqrt()
    }

    fn gathering(id: &str, embedding: Vec<f64>, diversity: u32) -> SignalEmbed {
        SignalEmbed {
            id: id.to_string(),
            label: "Gathering".to_string(),
            embedding,
            source_diversity: diversity,
        }
    }

    fn tension(id: &str, embedding: Vec<f64>, diversity: u32) -> SignalEmbed {
        SignalEmbed {
            id: id.to_string(),
            label: "Tension".to_string(),
            embedding,
            source_diversity: diversity,
        }
    }

    fn aid(id: &str, embedding: Vec<f64>, diversity: u32) -> SignalEmbed {
        SignalEmbed {
            id: id.to_string(),
            label: "Aid".to_string(),
            embedding,
            source_diversity: diversity,
        }
    }

    // --- cosine_similarity tests ---

    #[test]
    fn identical_vectors_similarity_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        let n = norm(&v);
        let sim = cosine_similarity(&v, &v, n, n);
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn orthogonal_vectors_similarity_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b, norm(&a), norm(&b));
        assert!(sim.abs() < 1e-10);
    }

    #[test]
    fn opposite_vectors_similarity_is_negative_one() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b, norm(&a), norm(&b));
        assert!((sim - (-1.0)).abs() < 1e-10);
    }

    #[test]
    fn zero_norm_returns_zero() {
        let a = vec![1.0, 2.0];
        let b = vec![0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b, norm(&a), 0.0), 0.0);
        assert_eq!(cosine_similarity(&b, &a, 0.0, norm(&a)), 0.0);
    }

    #[test]
    fn scaled_vectors_are_identical_similarity() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![2.0, 4.0, 6.0];
        let sim = cosine_similarity(&a, &b, norm(&a), norm(&b));
        assert!((sim - 1.0).abs() < 1e-10);
    }

    // --- compute_heats tests ---
    // Only Tensions radiate heat. Gatherings/Aids/Needs/Notices absorb heat
    // from nearby Tensions but never generate it.

    #[test]
    fn empty_signals_returns_empty() {
        let heats = compute_heats(&[], 0.7);
        assert!(heats.is_empty());
    }

    #[test]
    fn single_gathering_gets_zero_heat() {
        let signals = vec![gathering("a", vec![1.0, 0.0, 0.0], 5)];
        let heats = compute_heats(&signals, 0.7);
        assert_eq!(heats[0], 0.0);
    }

    #[test]
    fn single_tension_gets_zero_heat() {
        // A lone tension with no other tensions nearby gets zero heat
        let signals = vec![tension("a", vec![1.0, 0.0, 0.0], 5)];
        let heats = compute_heats(&signals, 0.7);
        assert_eq!(heats[0], 0.0);
    }

    #[test]
    fn gathering_near_tension_gets_heat() {
        // An Gathering semantically near a Tension absorbs heat from it.
        let signals = vec![
            gathering("protest", vec![1.0, 0.0, 0.0], 1),
            tension("ice_raids", vec![0.99, 0.1, 0.0], 5),
        ];
        let heats = compute_heats(&signals, 0.7);

        assert!(
            heats[0] > 0.5,
            "Gathering near tension should get heat, got {}",
            heats[0]
        );
        // Tension also gets heat from... no other tensions → zero
        // (only one tension in the graph)
        assert_eq!(heats[1], 0.0, "Lone tension has no tension neighbors");
    }

    #[test]
    fn gatherings_do_not_boost_each_other() {
        // 64 identical Gatherings with no Tension nearby → all get zero heat.
        // This is the Eventbrite blob scenario.
        let mut signals = Vec::new();
        for i in 0..64 {
            signals.push(gathering(&format!("meetup{i}"), vec![1.0, 0.0, 0.0], 3));
        }
        let heats = compute_heats(&signals, 0.7);

        for (i, h) in heats.iter().enumerate() {
            assert_eq!(
                *h, 0.0,
                "Gathering {i} should have zero heat without nearby tension"
            );
        }
    }

    #[test]
    fn tensions_corroborate_each_other() {
        // Multiple tensions about the same issue boost each other.
        // "ICE raids" and "immigrant fear" are semantically similar tensions.
        let signals = vec![
            tension("ice_raids", vec![1.0, 0.0, 0.0], 3),
            tension("immigrant_fear", vec![0.98, 0.15, 0.0], 5),
            tension("unrelated", vec![0.0, 0.0, 1.0], 2),
        ];
        let heats = compute_heats(&signals, 0.7);

        assert!(
            heats[0] > 0.5,
            "ICE raids should get heat from immigrant_fear, got {}",
            heats[0]
        );
        assert!(
            heats[1] > 0.5,
            "Immigrant fear should get heat from ICE raids, got {}",
            heats[1]
        );
        assert!(
            heats[2] < 0.01,
            "Unrelated tension gets no corroboration, got {}",
            heats[2]
        );
    }

    #[test]
    fn aid_near_tension_gets_heat_gatherings_dont() {
        // An Aid ("know your rights workshop") near a Tension gets heat.
        // A Gathering ("networking happy hour") far from any Tension gets nothing.
        let signals = vec![
            aid("workshop", vec![1.0, 0.0, 0.0], 1),
            tension("ice_raids", vec![0.98, 0.1, 0.0], 5),
            gathering("happy_hour", vec![0.0, 1.0, 0.0], 3),
        ];
        let heats = compute_heats(&signals, 0.7);

        assert!(
            heats[0] > 0.5,
            "Workshop near tension should get heat, got {}",
            heats[0]
        );
        assert!(
            heats[2] < 0.01,
            "Happy hour far from tension gets nothing, got {}",
            heats[2]
        );
    }

    #[test]
    fn heats_are_normalized_zero_to_one() {
        let signals = vec![
            gathering("a", vec![1.0, 0.0], 1),
            gathering("b", vec![1.0, 0.1], 1),
            tension("t", vec![1.0, 0.2], 8),
        ];
        let heats = compute_heats(&signals, 0.5);

        for h in &heats {
            assert!(*h >= 0.0 && *h <= 1.0, "Heat {} out of range", h);
        }
        // At least one signal should be 1.0 (the max)
        assert!(heats.iter().any(|h| (*h - 1.0).abs() < 1e-10));
    }

    #[test]
    fn threshold_filters_weak_similarity_to_tension() {
        // A Gathering and a Tension are somewhat similar but below a strict threshold
        let signals = vec![
            gathering("a", vec![1.0, 0.5], 5),
            tension("t", vec![0.5, 1.0], 5),
        ];
        // cos(a,t) ≈ 0.8 — below 0.9 threshold
        let heats = compute_heats(&signals, 0.9);
        assert_eq!(heats[0], 0.0);

        // Same signals with lower threshold — Gathering should get heat
        let heats = compute_heats(&signals, 0.7);
        assert!(heats[0] > 0.0);
    }

    #[test]
    fn gathering_blob_near_tension_all_get_heat() {
        // 64 Eventbrite events near a single Tension all absorb its heat.
        // But they don't boost each other — only the Tension radiates.
        let mut signals = Vec::new();
        for i in 0..64 {
            signals.push(gathering(&format!("meetup{i}"), vec![1.0, 0.0, 0.0], 1));
        }
        signals.push(tension("housing_crisis", vec![0.95, 0.2, 0.0], 8));
        let heats = compute_heats(&signals, 0.7);

        // All events should get roughly the same heat (from the one tension)
        let event_heats: Vec<f64> = heats[..64].to_vec();
        let min = event_heats.iter().cloned().fold(f64::MAX, f64::min);
        let max = event_heats.iter().cloned().fold(0.0_f64, f64::max);
        assert!(min > 0.0, "All events near tension should get heat");
        assert!(
            (max - min) < 0.1,
            "Gatherings equidistant from tension should get similar heat"
        );

        // The tension itself gets zero (no other tensions nearby)
        assert_eq!(heats[64], 0.0);
    }

    #[test]
    fn food_shelf_boosted_by_housing_tensions() {
        // A food shelf Aid near housing Tensions gets heat.
        // An unrelated park Gathering gets nothing.
        let signals = vec![
            aid("food_shelf", vec![0.85, 0.55, 0.0], 1),
            tension("housing_crisis", vec![0.8, 0.6, 0.0], 8),
            tension("rent_burden", vec![0.75, 0.65, 0.0], 6),
            tension("eviction_wave", vec![0.82, 0.58, 0.0], 4),
            gathering("park_event", vec![0.0, 0.0, 1.0], 3),
        ];
        let heats = compute_heats(&signals, 0.7);

        assert!(
            heats[0] > 0.5,
            "Food shelf near housing tensions should get heat, got {}",
            heats[0]
        );
        assert!(
            heats[4] < 0.01,
            "Park gathering far from any tension gets nothing, got {}",
            heats[4]
        );
    }

    #[test]
    fn diverse_tension_radiates_more_heat() {
        // Two gatherings equidistant from two tensions with different diversity.
        // The gathering near the high-diversity tension should get more heat.
        let signals = vec![
            gathering("a", vec![1.0, 0.0, 0.0], 1),
            tension("well_sourced", vec![0.99, 0.1, 0.0], 10),
            gathering("b", vec![0.0, 1.0, 0.0], 1),
            tension("single_source", vec![0.1, 0.99, 0.0], 1),
        ];
        let heats = compute_heats(&signals, 0.7);

        assert!(
            heats[0] > heats[2],
            "Gathering near diverse tension ({}) should outrank gathering near single-source tension ({})",
            heats[0],
            heats[2]
        );
    }

    #[test]
    fn duplicate_tensions_give_unearned_heat_to_each_other() {
        // Three near-identical youth violence tensions corroborate each other,
        // getting heat they didn't earn from independent sources.
        // After merging to one (with combined diversity), those self-corroboration
        // heat values disappear, and only the Aid absorbs heat.
        let signals_with_dupes = vec![
            aid("naz_tutoring", vec![0.9, 0.4, 0.0], 1),
            tension("youth_violence_1", vec![0.88, 0.47, 0.0], 1),
            tension("youth_violence_2", vec![0.87, 0.48, 0.0], 1),
            tension("youth_violence_3", vec![0.89, 0.46, 0.0], 1),
        ];
        let heats_duped = compute_heats(&signals_with_dupes, 0.7);

        // The duplicate tensions self-corroborate and get nonzero heat
        assert!(
            heats_duped[1] > 0.0,
            "Dup tension 1 gets unearned corroboration heat"
        );
        assert!(
            heats_duped[2] > 0.0,
            "Dup tension 2 gets unearned corroboration heat"
        );
        assert!(
            heats_duped[3] > 0.0,
            "Dup tension 3 gets unearned corroboration heat"
        );

        // After merging: one tension, no self-corroboration
        let signals_merged = vec![
            aid("naz_tutoring", vec![0.9, 0.4, 0.0], 1),
            tension("youth_violence", vec![0.88, 0.47, 0.0], 3),
        ];
        let heats_merged = compute_heats(&signals_merged, 0.7);

        // The merged tension gets zero (no other tension to corroborate with)
        assert_eq!(
            heats_merged[1], 0.0,
            "Merged tension should have zero heat (no corroboration)"
        );
        // The Aid still gets heat from the single tension
        assert_eq!(heats_merged[0], 1.0, "Aid gets all the heat after merge");
    }

    #[test]
    fn lone_tension_gets_heat_when_corroborated() {
        // A housing tension alone gets zero heat. Adding a corroborating
        // tension (from curiosity loop) gives the tension itself heat.
        let signals_alone = vec![
            tension("housing_crisis", vec![1.0, 0.0, 0.0], 3),
            aid("home_line", vec![0.95, 0.2, 0.0], 1),
        ];
        let heats_alone = compute_heats(&signals_alone, 0.7);
        assert_eq!(heats_alone[0], 0.0, "Lone tension should have zero heat");
        assert!(heats_alone[1] > 0.0, "Aid near tension should get heat");

        // Add a corroborating tension from the curiosity loop
        let signals_corroborated = vec![
            tension("housing_crisis", vec![1.0, 0.0, 0.0], 3),
            tension("rent_burden", vec![0.95, 0.15, 0.0], 2),
            aid("home_line", vec![0.95, 0.2, 0.0], 1),
        ];
        let heats_corroborated = compute_heats(&signals_corroborated, 0.7);
        // Now housing tension gets heat from rent_burden
        assert!(
            heats_corroborated[0] > 0.0,
            "Housing tension should get heat from corroborating rent tension, got {}",
            heats_corroborated[0]
        );
    }

    /// Integration test: run cause_heat against a live Neo4j instance.
    /// Run with: cargo test -p rootsignal-graph cause_heat_live -- --ignored
    #[tokio::test]
    #[ignore]
    async fn cause_heat_live() {
        let client = crate::GraphClient::connect("bolt://localhost:7687", "neo4j", "rootsignal")
            .await
            .expect("Failed to connect to Neo4j");

        compute_cause_heat(&client, 0.7)
            .await
            .expect("compute_cause_heat failed");

        // Verify: query top cause_heat signals
        let q = neo4rs::query(
            "MATCH (n)
             WHERE (n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension)
               AND n.cause_heat > 0
             RETURN n.title AS title, n.cause_heat AS heat,
                    n.source_diversity AS div, labels(n) AS labels
             ORDER BY n.cause_heat DESC
             LIMIT 15",
        );

        let mut stream = client.graph.execute(q).await.unwrap();
        println!("\n--- Top 15 signals by cause_heat ---");
        let mut count = 0;
        while let Some(row) = stream.next().await.unwrap() {
            let title: String = row.get("title").unwrap_or_default();
            let heat: f64 = row.get("heat").unwrap_or(0.0);
            let div: i64 = row.get("div").unwrap_or(0);
            let labels: Vec<String> = row.get("labels").unwrap_or_default();
            let label = labels
                .iter()
                .find(|l| *l != "Node")
                .cloned()
                .unwrap_or_default();
            println!("  heat={heat:.3}  div={div}  [{label}] {title}");
            count += 1;
        }
        assert!(count > 0, "Expected some signals with cause_heat > 0");

        // Also check that zero-heat signals exist (not everything should be hot)
        let q = neo4rs::query(
            "MATCH (n)
             WHERE (n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension)
               AND (n.cause_heat IS NULL OR n.cause_heat = 0)
             RETURN count(n) AS cnt",
        );
        let mut stream = client.graph.execute(q).await.unwrap();
        if let Some(row) = stream.next().await.unwrap() {
            let zero_count: i64 = row.get("cnt").unwrap_or(0);
            println!("\nSignals with zero/null cause_heat: {zero_count}");
        }
    }
}
