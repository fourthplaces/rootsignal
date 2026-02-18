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

    // 2. Precompute norms
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

    // 3. Compute cause_heat for each signal
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

    // 4. Normalize to 0.0–1.0
    let max_heat = heats.iter().cloned().fold(0.0_f64, f64::max);
    if max_heat > 0.0 {
        for h in &mut heats {
            *h /= max_heat;
        }
    }

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

    info!(updated, max_heat, "Cause heat computation complete");
    Ok(())
}

/// Cosine similarity with precomputed norms.
fn cosine_similarity(a: &[f64], b: &[f64], norm_a: f64, norm_b: f64) -> f64 {
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    dot / (norm_a * norm_b)
}
