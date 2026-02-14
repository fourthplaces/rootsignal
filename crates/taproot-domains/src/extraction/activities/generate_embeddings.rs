use anyhow::Result;
use pgvector::Vector;
use taproot_core::ServerDeps;
use uuid::Uuid;

/// Row data needed to build embedding text.
#[derive(Debug, sqlx::FromRow)]
struct EmbeddingInput {
    id: Uuid,
    title: String,
    description: Option<String>,
    location_text: Option<String>,
    entity_name: Option<String>,
}

/// Generate embeddings for listings that don't have one yet.
/// Returns the number of listings embedded.
pub async fn generate_embeddings(batch_size: i64, deps: &ServerDeps) -> Result<u32> {
    let pool = deps.pool();

    let rows = sqlx::query_as::<_, EmbeddingInput>(
        r#"
        SELECT l.id, l.title, l.description, l.location_text, e.name as entity_name
        FROM listings l
        LEFT JOIN entities e ON e.id = l.entity_id
        WHERE l.embedding IS NULL
        ORDER BY l.created_at ASC
        LIMIT $1
        "#,
    )
    .bind(batch_size)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    // Build embedding input texts: "title | description | location | org_name"
    let texts: Vec<String> = rows
        .iter()
        .map(|r| {
            let mut parts = vec![r.title.clone()];
            if let Some(desc) = &r.description {
                parts.push(desc.clone());
            }
            if let Some(loc) = &r.location_text {
                parts.push(loc.clone());
            }
            if let Some(org) = &r.entity_name {
                parts.push(org.clone());
            }
            parts.join(" | ")
        })
        .collect();

    let embeddings = deps.embedding_service.embed_batch(&texts).await?;

    let mut count = 0u32;
    for (row, embedding) in rows.iter().zip(embeddings.iter()) {
        let vector = Vector::from(embedding.clone());
        sqlx::query("UPDATE listings SET embedding = $1 WHERE id = $2")
            .bind(&vector)
            .bind(row.id)
            .execute(pool)
            .await?;
        count += 1;
    }

    tracing::info!(count, "Generated embeddings for listings");
    Ok(count)
}
