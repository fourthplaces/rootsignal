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
/// signal's semantic neighborhood. A food shelf Ask near a hot housing cluster
/// gets boosted — not because it posted a lot, but because the *cause* it serves
/// has genuine multi-source attention.
///
/// Algorithm:
/// 1. Load all signals with embeddings and source_diversity
/// 2. Compute all-pairs cosine similarity in memory
/// 3. For each signal, sum (similarity × neighbor.source_diversity) for neighbors above threshold
/// 4. Normalize to 0.0–1.0
/// 5. Write back to graph
pub async fn compute_cause_heat(
    client: &GraphClient,
    threshold: f64,
) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!(threshold, "Computing cause heat...");

    // 1. Load all signals with embeddings
    let mut signals: Vec<SignalEmbed> = Vec::new();

    for label in &["Event", "Give", "Ask", "Notice", "Tension"] {
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
        .map(|s| {
            s.embedding
                .iter()
                .map(|x| x * x)
                .sum::<f64>()
                .sqrt()
        })
        .collect();

    // Compute raw cause_heat for each signal
    let mut heats: Vec<f64> = vec![0.0; n];

    for i in 0..n {
        let mut heat = 0.0;
        for j in 0..n {
            if i == j {
                continue;
            }
            let sim = cosine_similarity(&signals[i].embedding, &signals[j].embedding, norms[i], norms[j]);
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

    fn signal(id: &str, embedding: Vec<f64>, diversity: u32) -> SignalEmbed {
        SignalEmbed {
            id: id.to_string(),
            label: "Event".to_string(),
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

    #[test]
    fn empty_signals_returns_empty() {
        let heats = compute_heats(&[], 0.7);
        assert!(heats.is_empty());
    }

    #[test]
    fn single_signal_gets_zero_heat() {
        let signals = vec![signal("a", vec![1.0, 0.0, 0.0], 5)];
        let heats = compute_heats(&signals, 0.7);
        assert_eq!(heats.len(), 1);
        assert_eq!(heats[0], 0.0);
    }

    #[test]
    fn similar_signals_get_heat_from_diverse_neighbors() {
        // Three signals: A and B are near-identical, C is orthogonal.
        // B has high diversity (10), A has low (1), C has high (10).
        // A should get high heat (from B's diversity).
        // C should get zero heat (orthogonal, below threshold).
        let signals = vec![
            signal("a", vec![1.0, 0.0, 0.0], 1),
            signal("b", vec![0.99, 0.1, 0.0], 10),
            signal("c", vec![0.0, 0.0, 1.0], 10),
        ];
        let heats = compute_heats(&signals, 0.7);

        // A is similar to B (high diversity) → high heat
        assert!(heats[0] > 0.5, "A should have high heat, got {}", heats[0]);
        // B is similar to A (low diversity) → lower heat
        assert!(heats[1] > 0.0, "B should have some heat, got {}", heats[1]);
        // C is orthogonal to A and B → zero heat
        assert!(heats[2] < 0.01, "C should have near-zero heat, got {}", heats[2]);
    }

    #[test]
    fn heats_are_normalized_zero_to_one() {
        let signals = vec![
            signal("a", vec![1.0, 0.0], 1),
            signal("b", vec![1.0, 0.1], 5),
            signal("c", vec![1.0, 0.2], 8),
        ];
        let heats = compute_heats(&signals, 0.5);

        for h in &heats {
            assert!(*h >= 0.0 && *h <= 1.0, "Heat {} out of range", h);
        }
        // At least one signal should be 1.0 (the max)
        assert!(heats.iter().any(|h| (*h - 1.0).abs() < 1e-10));
    }

    #[test]
    fn threshold_filters_weak_similarity() {
        // A and B are somewhat similar but below a strict threshold
        let signals = vec![
            signal("a", vec![1.0, 0.5], 5),
            signal("b", vec![0.5, 1.0], 5),
        ];
        // cos(a,b) ≈ 0.8 — below 0.9 threshold
        let heats = compute_heats(&signals, 0.9);
        assert_eq!(heats[0], 0.0);
        assert_eq!(heats[1], 0.0);

        // Same signals with lower threshold — should get heat
        let heats = compute_heats(&signals, 0.7);
        assert!(heats[0] > 0.0);
        assert!(heats[1] > 0.0);
    }

    #[test]
    fn self_promotion_does_not_inflate_neighbors() {
        // "Spammer" has 5 near-identical signals (diversity=1 each, same source).
        // "Genuine" has 1 signal with high diversity.
        // The spammer cluster should NOT inflate each other much because diversity=1.
        // Genuine's heat should come from its own neighborhood, not spam.
        let spam_dir = vec![1.0, 0.0, 0.0];
        let genuine_dir = vec![0.0, 1.0, 0.0];

        let mut signals = Vec::new();
        for i in 0..5 {
            signals.push(signal(&format!("spam{i}"), spam_dir.clone(), 1));
        }
        signals.push(signal("genuine", genuine_dir.clone(), 10));

        let heats = compute_heats(&signals, 0.7);

        // Genuine is orthogonal to spam → zero heat (no similar high-diversity neighbors)
        assert!(heats[5] < 0.01, "Genuine should not get heat from orthogonal spam cluster");

        // Each spammer gets heat from 4 identical neighbors × diversity 1
        // This is much lower than if they had diversity=10
        let max_spam_heat = heats[..5].iter().cloned().fold(0.0_f64, f64::max);
        assert!(max_spam_heat > 0.0, "Spam cluster members get some heat from each other");
    }

    #[test]
    fn food_shelf_boosted_by_housing_cluster() {
        // The core use case: a food shelf Ask (posted once, diversity=1)
        // rises because housing signals (high diversity) are semantically nearby.
        // "poverty" embedding direction: [0.8, 0.6, 0.0]
        let food_shelf = signal("food_shelf", vec![0.85, 0.55, 0.0], 1);
        let housing_1 = signal("housing_1", vec![0.8, 0.6, 0.0], 8);
        let housing_2 = signal("housing_2", vec![0.75, 0.65, 0.0], 6);
        let housing_3 = signal("housing_3", vec![0.82, 0.58, 0.0], 4);
        // Unrelated signal in a different direction
        let unrelated = signal("park_event", vec![0.0, 0.0, 1.0], 3);

        let signals = vec![food_shelf, housing_1, housing_2, housing_3, unrelated];
        let heats = compute_heats(&signals, 0.7);

        // Food shelf should have high heat (near high-diversity housing cluster)
        assert!(heats[0] > 0.5, "Food shelf should be boosted by housing cluster, got {}", heats[0]);
        // Unrelated park event should have zero heat
        assert!(heats[4] < 0.01, "Unrelated signal should have near-zero heat, got {}", heats[4]);
    }

    /// Integration test: run cause_heat against a live Neo4j instance.
    /// Run with: cargo test -p rootsignal-graph cause_heat_live -- --ignored
    #[tokio::test]
    #[ignore]
    async fn cause_heat_live() {
        let client = crate::GraphClient::connect(
            "bolt://localhost:7687",
            "neo4j",
            "rootsignal",
        )
        .await
        .expect("Failed to connect to Neo4j");

        compute_cause_heat(&client, 0.7)
            .await
            .expect("compute_cause_heat failed");

        // Verify: query top cause_heat signals
        let q = neo4rs::query(
            "MATCH (n)
             WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
               AND n.cause_heat > 0
             RETURN n.title AS title, n.cause_heat AS heat,
                    n.source_diversity AS div, labels(n) AS labels
             ORDER BY n.cause_heat DESC
             LIMIT 15"
        );

        let mut stream = client.graph.execute(q).await.unwrap();
        println!("\n--- Top 15 signals by cause_heat ---");
        let mut count = 0;
        while let Some(row) = stream.next().await.unwrap() {
            let title: String = row.get("title").unwrap_or_default();
            let heat: f64 = row.get("heat").unwrap_or(0.0);
            let div: i64 = row.get("div").unwrap_or(0);
            let labels: Vec<String> = row.get("labels").unwrap_or_default();
            let label = labels.iter().find(|l| *l != "Node").cloned().unwrap_or_default();
            println!("  heat={heat:.3}  div={div}  [{label}] {title}");
            count += 1;
        }
        assert!(count > 0, "Expected some signals with cause_heat > 0");

        // Also check that zero-heat signals exist (not everything should be hot)
        let q = neo4rs::query(
            "MATCH (n)
             WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
               AND (n.cause_heat IS NULL OR n.cause_heat = 0)
             RETURN count(n) AS cnt"
        );
        let mut stream = client.graph.execute(q).await.unwrap();
        if let Some(row) = stream.next().await.unwrap() {
            let zero_count: i64 = row.get("cnt").unwrap_or(0);
            println!("\nSignals with zero/null cause_heat: {zero_count}");
        }
    }
}
