use anyhow::Result;
use sqlx::PgPool;
use std::time::Instant;

use super::types::*;

/// Execute a hybrid search combining semantic similarity (pgvector) with
/// full-text search (tsvector) using Reciprocal Rank Fusion (RRF).
///
/// When query text/embedding are present, runs both CTE pipelines and fuses ranks.
/// When absent, falls back to filter-only mode with created_at ordering.
pub async fn hybrid_search(params: &HybridSearchParams, pool: &PgPool) -> Result<SearchResponse> {
    let start = Instant::now();

    let has_semantic = params.query_embedding.is_some();
    let has_fts = params.query_text.is_some();

    let (results, mode) = if has_semantic || has_fts {
        let results = hybrid_ranked_search(params, pool).await?;
        let mode = if has_semantic {
            SearchMode::SemanticPlusFts
        } else {
            SearchMode::FtsOnly
        };
        (results, mode)
    } else {
        let results = filters_only_search(params, pool).await?;
        (results, SearchMode::FiltersOnly)
    };

    let total_estimate = results.len() as i64;
    let took_ms = start.elapsed().as_millis() as u64;

    Ok(SearchResponse {
        results,
        total_estimate,
        mode,
        took_ms,
    })
}

/// RRF-ranked hybrid search using CTEs for semantic + FTS pipelines.
async fn hybrid_ranked_search(
    params: &HybridSearchParams,
    pool: &PgPool,
) -> Result<Vec<SearchResultRow>> {
    let mut qb = sqlx::QueryBuilder::new("");

    // -- CTE: semantic candidates (top 200 by cosine similarity)
    let has_semantic = params.query_embedding.is_some();
    let has_fts = params.query_text.is_some();

    if has_semantic {
        qb.push(
            "WITH sem AS ( \
                SELECT e.embeddable_id AS signal_id, \
                    (e.embedding <=> ",
        );
        qb.push_bind(params.query_embedding.as_ref().unwrap().clone());
        qb.push(
            ") AS distance \
                FROM embeddings e \
                WHERE e.embeddable_type = 'signal' AND e.locale = 'en' \
                ORDER BY e.embedding <=> ",
        );
        qb.push_bind(params.query_embedding.as_ref().unwrap().clone());
        qb.push(" LIMIT 200 ) ");
    }

    // -- CTE: FTS candidates (top 200 by ts_rank)
    if has_fts {
        if has_semantic {
            qb.push(", ");
        } else {
            qb.push("WITH ");
        }
        qb.push("fts AS ( SELECT s.id AS signal_id, ts_rank(s.search_vector, websearch_to_tsquery('english', ");
        qb.push_bind(params.query_text.as_ref().unwrap().clone());
        qb.push(
            ")) AS rank \
                FROM signals s \
                WHERE s.search_vector @@ websearch_to_tsquery('english', ",
        );
        qb.push_bind(params.query_text.as_ref().unwrap().clone());
        qb.push(
            ") \
                ORDER BY rank DESC \
                LIMIT 200 ) ",
        );
    }

    // -- CTE: RRF ranking via FULL OUTER JOIN
    if has_semantic && has_fts {
        qb.push(
            ", ranked AS ( \
                SELECT COALESCE(sem.signal_id, fts.signal_id) AS signal_id, \
                    sem.distance AS sem_distance, \
                    fts.rank AS fts_rank, \
                    ROW_NUMBER() OVER (ORDER BY sem.distance ASC NULLS LAST) AS sem_rn, \
                    ROW_NUMBER() OVER (ORDER BY fts.rank DESC NULLS LAST) AS fts_rn \
                FROM sem FULL OUTER JOIN fts ON sem.signal_id = fts.signal_id \
            ), scored AS ( \
                SELECT signal_id, sem_distance, fts_rank, \
                    (1.0 / (60 + sem_rn)) + (1.0 / (60 + fts_rn)) AS rrf_score \
                FROM ranked \
            ) ",
        );
    } else if has_semantic {
        qb.push(
            ", scored AS ( \
                SELECT signal_id, distance AS sem_distance, NULL::real AS fts_rank, \
                    (1.0 - distance) AS rrf_score \
                FROM sem \
            ) ",
        );
    } else {
        // FTS only
        qb.push(
            ", scored AS ( \
                SELECT signal_id, NULL::double precision AS sem_distance, rank AS fts_rank, \
                    rank::double precision AS rrf_score \
                FROM fts \
            ) ",
        );
    }

    // -- CTE: cluster dedup
    qb.push(
        ", cluster_reps AS ( \
            SELECT representative_id AS signal_id FROM clusters WHERE cluster_type = 'entity' \
        ) ",
    );

    // -- Main SELECT
    qb.push("SELECT s.id, ");
    qb.push("s.content AS title, ");
    qb.push("s.about AS description, ");
    qb.push("'active' AS status, ");
    qb.push("s.entity_id, ");
    qb.push("e.name AS entity_name, e.entity_type, ");
    qb.push("s.source_url, NULL::text AS location_text, ");
    qb.push("s.created_at, s.in_language, ");
    qb.push("s.in_language AS locale, ");
    qb.push("false AS is_fallback, ");
    qb.push(
        "CASE WHEN sc.sem_distance IS NOT NULL THEN (1.0 - sc.sem_distance) ELSE NULL END AS semantic_score, ",
    );
    qb.push("sc.fts_rank::double precision AS text_score, ");
    qb.push("sc.rrf_score AS combined_score, ");
    qb.push("NULL::double precision AS distance_miles ");

    // -- FROM + JOINs
    qb.push("FROM scored sc ");
    qb.push("JOIN signals s ON s.id = sc.signal_id ");
    qb.push("LEFT JOIN entities e ON e.id = s.entity_id ");

    // Temporal join
    if params.temporal.happening_on.is_some()
        || params.temporal.happening_between.is_some()
        || params.temporal.day_of_week.is_some()
    {
        qb.push(
            "JOIN schedules sch ON sch.scheduleable_type = 'signal' AND sch.scheduleable_id = s.id ",
        );
    }

    // Geo join
    if params.filters.lat.is_some() && params.filters.lng.is_some() {
        qb.push(
            "LEFT JOIN locationables loc ON loc.locatable_type = 'signal' AND loc.locatable_id = s.id \
             LEFT JOIN locations lp ON lp.id = loc.location_id ",
        );
    }

    // -- WHERE
    qb.push("WHERE 1=1 ");

    // Cluster dedup
    qb.push(
        "AND (s.id IN (SELECT signal_id FROM cluster_reps) \
         OR NOT EXISTS (SELECT 1 FROM cluster_items ci WHERE ci.item_type = 'signal' AND ci.item_id = s.id AND ci.cluster_type = 'entity')) ",
    );

    // Geo filter with bounding-box pre-filter for B-tree index usage
    if let (Some(lat), Some(lng), Some(radius_km)) = (
        params.filters.lat,
        params.filters.lng,
        params.filters.radius_km,
    ) {
        let lat_delta = radius_km / 111.0;
        qb.push("AND lp.latitude IS NOT NULL ");
        // Bounding box pre-filter (uses B-tree indexes)
        qb.push("AND lp.latitude BETWEEN ");
        qb.push_bind(lat - lat_delta);
        qb.push(" AND ");
        qb.push_bind(lat + lat_delta);
        qb.push(" AND lp.longitude BETWEEN ");
        qb.push_bind(lng - radius_km / (111.0 * (lat.to_radians().cos())));
        qb.push(" AND ");
        qb.push_bind(lng + radius_km / (111.0 * (lat.to_radians().cos())));
        // Precise haversine filter
        qb.push(" AND (6371 * acos(cos(radians(");
        qb.push_bind(lat);
        qb.push(")) * cos(radians(lp.latitude)) * cos(radians(lp.longitude) - radians(");
        qb.push_bind(lng);
        qb.push(")) + sin(radians(");
        qb.push_bind(lat);
        qb.push(")) * sin(radians(lp.latitude)))) <= ");
        qb.push_bind(radius_km);
        qb.push(" ");
    }

    // Temporal filters
    append_temporal_filters(&mut qb, &params.temporal);

    // ORDER + LIMIT
    qb.push("ORDER BY sc.rrf_score DESC ");
    qb.push("LIMIT ");
    qb.push_bind(params.limit);
    qb.push(" OFFSET ");
    qb.push_bind(params.offset);

    let rows = qb
        .build_query_as::<SearchResultRow>()
        .fetch_all(pool)
        .await?;

    Ok(rows)
}

/// Filter-only search when no query text is provided.
async fn filters_only_search(
    params: &HybridSearchParams,
    pool: &PgPool,
) -> Result<Vec<SearchResultRow>> {
    let mut qb = sqlx::QueryBuilder::new(
        "WITH cluster_reps AS ( \
            SELECT representative_id AS signal_id FROM clusters WHERE cluster_type = 'entity' \
        ) SELECT s.id, ",
    );

    qb.push("s.content AS title, ");
    qb.push("s.about AS description, ");
    qb.push("'active' AS status, ");
    qb.push("s.entity_id, ");
    qb.push("e.name AS entity_name, e.entity_type, ");
    qb.push("s.source_url, NULL::text AS location_text, ");
    qb.push("s.created_at, s.in_language, ");
    qb.push("s.in_language AS locale, ");
    qb.push("false AS is_fallback, ");
    qb.push("NULL::double precision AS semantic_score, ");
    qb.push("NULL::double precision AS text_score, ");
    qb.push("s.confidence::double precision AS combined_score, ");
    qb.push("NULL::double precision AS distance_miles ");

    // FROM + JOINs
    qb.push("FROM signals s ");
    qb.push("LEFT JOIN entities e ON e.id = s.entity_id ");

    // Temporal join
    if params.temporal.happening_on.is_some()
        || params.temporal.happening_between.is_some()
        || params.temporal.day_of_week.is_some()
    {
        qb.push(
            "JOIN schedules sch ON sch.scheduleable_type = 'signal' AND sch.scheduleable_id = s.id ",
        );
    }

    // Geo join
    if params.filters.lat.is_some() && params.filters.lng.is_some() {
        qb.push(
            "LEFT JOIN locationables loc ON loc.locatable_type = 'signal' AND loc.locatable_id = s.id \
             LEFT JOIN locations lp ON lp.id = loc.location_id ",
        );
    }

    // WHERE
    qb.push("WHERE 1=1 ");

    // Cluster dedup
    qb.push(
        "AND (s.id IN (SELECT signal_id FROM cluster_reps) \
         OR NOT EXISTS (SELECT 1 FROM cluster_items ci WHERE ci.item_type = 'signal' AND ci.item_id = s.id AND ci.cluster_type = 'entity')) ",
    );

    // Geo filter with bounding-box pre-filter for B-tree index usage
    if let (Some(lat), Some(lng), Some(radius_km)) = (
        params.filters.lat,
        params.filters.lng,
        params.filters.radius_km,
    ) {
        let lat_delta = radius_km / 111.0;
        qb.push("AND lp.latitude IS NOT NULL ");
        // Bounding box pre-filter
        qb.push("AND lp.latitude BETWEEN ");
        qb.push_bind(lat - lat_delta);
        qb.push(" AND ");
        qb.push_bind(lat + lat_delta);
        qb.push(" AND lp.longitude BETWEEN ");
        qb.push_bind(lng - radius_km / (111.0 * (lat.to_radians().cos())));
        qb.push(" AND ");
        qb.push_bind(lng + radius_km / (111.0 * (lat.to_radians().cos())));
        // Precise haversine filter
        qb.push(" AND (6371 * acos(cos(radians(");
        qb.push_bind(lat);
        qb.push(")) * cos(radians(lp.latitude)) * cos(radians(lp.longitude) - radians(");
        qb.push_bind(lng);
        qb.push(")) + sin(radians(");
        qb.push_bind(lat);
        qb.push(")) * sin(radians(lp.latitude)))) <= ");
        qb.push_bind(radius_km);
        qb.push(" ");
    }

    // Temporal filters
    append_temporal_filters(&mut qb, &params.temporal);

    // ORDER + LIMIT
    qb.push("ORDER BY s.confidence DESC NULLS LAST, s.created_at DESC ");
    qb.push("LIMIT ");
    qb.push_bind(params.limit);
    qb.push(" OFFSET ");
    qb.push_bind(params.offset);

    let rows = qb
        .build_query_as::<SearchResultRow>()
        .fetch_all(pool)
        .await?;

    Ok(rows)
}

/// Append temporal WHERE clauses to a QueryBuilder.
fn append_temporal_filters(
    qb: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    temporal: &TemporalFilter,
) {
    if let Some(date) = &temporal.happening_on {
        qb.push("AND sch.valid_from::date <= ");
        qb.push_bind(*date);
        qb.push(" AND (sch.valid_through IS NULL OR sch.valid_through::date >= ");
        qb.push_bind(*date);
        qb.push(") ");
    }

    if let Some((start, end)) = &temporal.happening_between {
        qb.push("AND sch.valid_from::date <= ");
        qb.push_bind(*end);
        qb.push(" AND (sch.valid_through IS NULL OR sch.valid_through::date >= ");
        qb.push_bind(*start);
        qb.push(") ");
    }

    if let Some(dow) = &temporal.day_of_week {
        // Map iCal day codes to Postgres DOW (0=Sunday)
        let pg_dow = match dow.as_str() {
            "SU" => 0,
            "MO" => 1,
            "TU" => 2,
            "WE" => 3,
            "TH" => 4,
            "FR" => 5,
            "SA" => 6,
            _ => return,
        };
        qb.push("AND EXTRACT(DOW FROM sch.valid_from) = ");
        qb.push_bind(pg_dow);
        qb.push(" ");
    }
}
