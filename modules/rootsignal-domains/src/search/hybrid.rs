use anyhow::Result;
use sqlx::PgPool;
use std::time::Instant;

use super::types::*;

/// Execute a hybrid search combining semantic similarity (pgvector) with
/// full-text search (tsvector) using Reciprocal Rank Fusion (RRF).
///
/// When query text/embedding are present, runs both CTE pipelines and fuses ranks.
/// When absent, falls back to filter-only mode with relevance_score ordering.
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
                SELECT e.embeddable_id AS listing_id, \
                    (e.embedding <=> ",
        );
        qb.push_bind(params.query_embedding.as_ref().unwrap().clone());
        qb.push(
            ") AS distance \
                FROM embeddings e \
                WHERE e.embeddable_type = 'listing' AND e.locale = 'en' \
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
        qb.push("fts AS ( SELECT l.id AS listing_id, ts_rank(l.search_vector, websearch_to_tsquery('english', ");
        qb.push_bind(params.query_text.as_ref().unwrap().clone());
        qb.push(
            ")) AS rank \
                FROM listings l \
                WHERE l.search_vector @@ websearch_to_tsquery('english', ",
        );
        qb.push_bind(params.query_text.as_ref().unwrap().clone());
        qb.push(
            ") AND l.status = 'active' \
                ORDER BY rank DESC \
                LIMIT 200 ) ",
        );
    }

    // -- CTE: RRF ranking via FULL OUTER JOIN
    if has_semantic && has_fts {
        qb.push(
            ", ranked AS ( \
                SELECT COALESCE(sem.listing_id, fts.listing_id) AS listing_id, \
                    sem.distance AS sem_distance, \
                    fts.rank AS fts_rank, \
                    ROW_NUMBER() OVER (ORDER BY sem.distance ASC NULLS LAST) AS sem_rn, \
                    ROW_NUMBER() OVER (ORDER BY fts.rank DESC NULLS LAST) AS fts_rn \
                FROM sem FULL OUTER JOIN fts ON sem.listing_id = fts.listing_id \
            ), scored AS ( \
                SELECT listing_id, sem_distance, fts_rank, \
                    (1.0 / (60 + sem_rn)) + (1.0 / (60 + fts_rn)) AS rrf_score \
                FROM ranked \
            ) ",
        );
    } else if has_semantic {
        qb.push(
            ", scored AS ( \
                SELECT listing_id, distance AS sem_distance, NULL::real AS fts_rank, \
                    (1.0 - distance) AS rrf_score \
                FROM sem \
            ) ",
        );
    } else {
        // FTS only
        qb.push(
            ", scored AS ( \
                SELECT listing_id, NULL::double precision AS sem_distance, rank AS fts_rank, \
                    rank::double precision AS rrf_score \
                FROM fts \
            ) ",
        );
    }

    // -- CTE: cluster dedup
    qb.push(
        ", cluster_reps AS ( \
            SELECT DISTINCT ON (c.id) ci.item_id AS listing_id \
            FROM clusters c \
            JOIN cluster_items ci ON ci.cluster_id = c.id AND ci.item_type = 'listing' \
            JOIN listings l ON l.id = ci.item_id \
            WHERE c.cluster_type = 'listing' AND l.status = 'active' \
            ORDER BY c.id, (ci.item_id = c.representative_id) DESC, ci.similarity_score DESC NULLS LAST \
        ) ",
    );

    // -- Main SELECT with translation fallback
    qb.push("SELECT l.id, ");
    qb.push("COALESCE(t_title.content, en_title.content, l.title) AS title, ");
    qb.push("COALESCE(t_desc.content, en_desc.content, l.description) AS description, ");
    qb.push("l.status, ");
    qb.push("l.entity_id, ");
    qb.push("e.name AS entity_name, e.entity_type, ");
    qb.push("l.source_url, l.location_text, ");
    qb.push("l.created_at, l.in_language, ");
    qb.push("CASE WHEN t_title.content IS NOT NULL THEN ");
    qb.push_bind(params.locale.clone());
    qb.push(" WHEN en_title.content IS NOT NULL THEN 'en' ELSE l.in_language END AS locale, ");
    qb.push("CASE WHEN t_title.content IS NOT NULL THEN false ELSE true END AS is_fallback, ");
    qb.push(
        "CASE WHEN sc.sem_distance IS NOT NULL THEN (1.0 - sc.sem_distance) ELSE NULL END AS semantic_score, ",
    );
    qb.push("sc.fts_rank::double precision AS text_score, ");
    qb.push("sc.rrf_score AS combined_score, ");
    qb.push("NULL::double precision AS distance_miles ");

    // -- FROM + JOINs
    qb.push("FROM scored sc ");
    qb.push("JOIN listings l ON l.id = sc.listing_id ");
    qb.push("LEFT JOIN entities e ON e.id = l.entity_id ");

    // Translation joins
    crate::query_helpers::append_translation_joins(&mut qb, &params.locale, "l");

    // Temporal join
    if params.temporal.happening_on.is_some()
        || params.temporal.happening_between.is_some()
        || params.temporal.day_of_week.is_some()
    {
        qb.push(
            "JOIN schedules s ON s.scheduleable_type = 'listing' AND s.scheduleable_id = l.id ",
        );
    }

    // Geo join
    if params.filters.lat.is_some() && params.filters.lng.is_some() {
        qb.push(
            "LEFT JOIN locationables loc ON loc.locatable_type = 'listing' AND loc.locatable_id = l.id \
             LEFT JOIN locations lp ON lp.id = loc.location_id ",
        );
    }

    // -- WHERE
    qb.push("WHERE l.status = 'active' AND (l.expires_at IS NULL OR l.expires_at > NOW()) ");

    // Cluster dedup
    qb.push(
        "AND (l.id IN (SELECT listing_id FROM cluster_reps) \
         OR l.id NOT IN (SELECT item_id FROM cluster_items WHERE item_type = 'listing')) ",
    );

    // Tag filters
    append_tag_filters(&mut qb, &params.filters);

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
            SELECT DISTINCT ON (c.id) ci.item_id AS listing_id \
            FROM clusters c \
            JOIN cluster_items ci ON ci.cluster_id = c.id AND ci.item_type = 'listing' \
            JOIN listings l ON l.id = ci.item_id \
            WHERE c.cluster_type = 'listing' AND l.status = 'active' \
            ORDER BY c.id, (ci.item_id = c.representative_id) DESC, ci.similarity_score DESC NULLS LAST \
        ) SELECT l.id, ",
    );

    // Translation fallback
    qb.push("COALESCE(t_title.content, en_title.content, l.title) AS title, ");
    qb.push("COALESCE(t_desc.content, en_desc.content, l.description) AS description, ");
    qb.push("l.status, ");
    qb.push("l.entity_id, ");
    qb.push("e.name AS entity_name, e.entity_type, ");
    qb.push("l.source_url, l.location_text, ");
    qb.push("l.created_at, l.in_language, ");
    qb.push("CASE WHEN t_title.content IS NOT NULL THEN ");
    qb.push_bind(params.locale.clone());
    qb.push(" WHEN en_title.content IS NOT NULL THEN 'en' ELSE l.in_language END AS locale, ");
    qb.push("CASE WHEN t_title.content IS NOT NULL THEN false ELSE true END AS is_fallback, ");
    qb.push("NULL::double precision AS semantic_score, ");
    qb.push("NULL::double precision AS text_score, ");
    qb.push("COALESCE(l.relevance_score, 0)::double precision AS combined_score, ");
    qb.push("NULL::double precision AS distance_miles ");

    // FROM + JOINs
    qb.push("FROM listings l ");
    qb.push("LEFT JOIN entities e ON e.id = l.entity_id ");

    crate::query_helpers::append_translation_joins(&mut qb, &params.locale, "l");

    // Temporal join
    if params.temporal.happening_on.is_some()
        || params.temporal.happening_between.is_some()
        || params.temporal.day_of_week.is_some()
    {
        qb.push(
            "JOIN schedules s ON s.scheduleable_type = 'listing' AND s.scheduleable_id = l.id ",
        );
    }

    // Geo join
    if params.filters.lat.is_some() && params.filters.lng.is_some() {
        qb.push(
            "LEFT JOIN locationables loc ON loc.locatable_type = 'listing' AND loc.locatable_id = l.id \
             LEFT JOIN locations lp ON lp.id = loc.location_id ",
        );
    }

    // WHERE
    qb.push("WHERE l.status = 'active' AND (l.expires_at IS NULL OR l.expires_at > NOW()) ");

    // Cluster dedup
    qb.push(
        "AND (l.id IN (SELECT listing_id FROM cluster_reps) \
         OR l.id NOT IN (SELECT item_id FROM cluster_items WHERE item_type = 'listing')) ",
    );

    // Tag filters
    append_tag_filters(&mut qb, &params.filters);

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
    qb.push("ORDER BY l.relevance_score DESC NULLS LAST, l.created_at DESC ");
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

/// Append tag EXISTS subqueries to a QueryBuilder.
fn append_tag_filters(
    qb: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filters: &crate::listings::ListingFilters,
) {
    crate::query_helpers::append_tag_filters(qb, filters, "l");
}

/// Append temporal WHERE clauses to a QueryBuilder.
fn append_temporal_filters(
    qb: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    temporal: &TemporalFilter,
) {
    if let Some(date) = &temporal.happening_on {
        qb.push("AND s.valid_from::date <= ");
        qb.push_bind(*date);
        qb.push(" AND (s.valid_through IS NULL OR s.valid_through::date >= ");
        qb.push_bind(*date);
        qb.push(") ");
    }

    if let Some((start, end)) = &temporal.happening_between {
        qb.push("AND s.valid_from::date <= ");
        qb.push_bind(*end);
        qb.push(" AND (s.valid_through IS NULL OR s.valid_through::date >= ");
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
        qb.push("AND EXTRACT(DOW FROM s.valid_from) = ");
        qb.push_bind(pg_dow);
        qb.push(" ");
    }
}
