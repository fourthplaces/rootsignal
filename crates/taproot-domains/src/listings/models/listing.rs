use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Listing {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub entity_id: Option<Uuid>,
    pub service_id: Option<Uuid>,
    pub source_url: Option<String>,
    pub location_text: Option<String>,
    pub source_locale: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub freshness_score: f32,
    pub relevance_score: Option<i32>,
    pub relevance_breakdown: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Listing {
    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM listings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_active(limit: i64, offset: i64, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM listings
            WHERE status = 'active'
            ORDER BY created_at DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_recent(days: i32, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT DISTINCT l.* FROM listings l
            JOIN schedules s ON s.scheduleable_type = 'listing' AND s.scheduleable_id = l.id
            WHERE l.status = 'active'
              AND s.valid_from > NOW() - ($1 || ' days')::INTERVAL
            ORDER BY l.created_at DESC
            "#,
        )
        .bind(days)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn random_sample(count: i64, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM listings
            WHERE status = 'active'
            ORDER BY RANDOM()
            LIMIT $1
            "#,
        )
        .bind(count)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn count_active(pool: &PgPool) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM listings WHERE status = 'active'",
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Dynamic filtered query using tag-based filters, geo, temporal, and pagination.
    pub async fn find_filtered(filters: &ListingFilters, pool: &PgPool) -> Result<Vec<Self>> {
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT DISTINCT l.* FROM listings l "
        );

        // If geo filter, join locations
        if filters.lat.is_some() && filters.lng.is_some() {
            qb.push(
                "LEFT JOIN locationables loc ON loc.locatable_type = 'listing' AND loc.locatable_id = l.id \
                 LEFT JOIN locations lp ON lp.id = loc.location_id "
            );
        }

        qb.push("WHERE l.status = 'active' ");

        // Tag-based filters — uniform EXISTS subquery pattern
        let tag_filters: Vec<(&str, &Option<String>)> = vec![
            ("signal_domain", &filters.signal_domain),
            ("audience_role", &filters.audience_role),
            ("category", &filters.category),
            ("listing_type", &filters.listing_type),
            ("urgency", &filters.urgency),
            ("confidence", &filters.confidence),
            ("capacity_status", &filters.capacity_status),
            ("radius_relevant", &filters.radius_relevant),
            ("population", &filters.population),
        ];

        for (kind, value) in &tag_filters {
            if let Some(val) = value {
                qb.push("AND EXISTS (SELECT 1 FROM taggables tg JOIN tags t ON t.id = tg.tag_id WHERE tg.taggable_type = 'listing' AND tg.taggable_id = l.id AND t.kind = ");
                qb.push_bind(*kind);
                qb.push(" AND t.value = ");
                qb.push_bind(val.clone());
                qb.push(") ");
            }
        }

        // Geo filter (Haversine)
        if let (Some(lat), Some(lng), Some(radius_km)) = (filters.lat, filters.lng, filters.radius_km) {
            qb.push("AND lp.latitude IS NOT NULL AND (6371 * acos(cos(radians(");
            qb.push_bind(lat);
            qb.push(")) * cos(radians(lp.latitude)) * cos(radians(lp.longitude) - radians(");
            qb.push_bind(lng);
            qb.push(")) + sin(radians(");
            qb.push_bind(lat);
            qb.push(")) * sin(radians(lp.latitude)))) <= ");
            qb.push_bind(radius_km);
            qb.push(" ");
        }

        // Hotspot filter
        if let Some(hotspot_id) = &filters.hotspot_id {
            qb.push("AND EXISTS (SELECT 1 FROM locationables loc2 JOIN locations lp2 ON lp2.id = loc2.location_id, hotspots h WHERE loc2.locatable_type = 'listing' AND loc2.locatable_id = l.id AND h.id = ");
            qb.push_bind(*hotspot_id);
            qb.push(" AND lp2.latitude IS NOT NULL AND (6371000 * acos(cos(radians(h.center_lat)) * cos(radians(lp2.latitude)) * cos(radians(lp2.longitude) - radians(h.center_lng)) + sin(radians(h.center_lat)) * sin(radians(lp2.latitude)))) <= h.radius_meters) ");
        }

        // Temporal
        if let Some(since) = &filters.since {
            qb.push("AND l.created_at >= ");
            qb.push_bind(*since);
            qb.push(" ");
        }

        // Exclude expired
        qb.push("AND (l.expires_at IS NULL OR l.expires_at > NOW()) ");

        // Order
        qb.push("ORDER BY l.relevance_score DESC NULLS LAST, l.created_at DESC ");

        // Pagination
        let limit = filters.limit.unwrap_or(50);
        let offset = filters.offset.unwrap_or(0);
        qb.push("LIMIT ");
        qb.push_bind(limit);
        qb.push(" OFFSET ");
        qb.push_bind(offset);

        qb.build_query_as::<Self>()
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }
}

/// Filter parameters for listing queries. All tag-based filters use uniform EXISTS subqueries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListingFilters {
    // Tag-based filters
    pub signal_domain: Option<String>,
    pub audience_role: Option<String>,
    pub category: Option<String>,
    pub listing_type: Option<String>,
    pub urgency: Option<String>,
    pub confidence: Option<String>,
    pub capacity_status: Option<String>,
    pub radius_relevant: Option<String>,
    pub population: Option<String>,
    // Geo
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub radius_km: Option<f64>,
    pub hotspot_id: Option<Uuid>,
    // Zip-based geo
    pub zip_code: Option<String>,
    pub radius_miles: Option<f64>,
    // Temporal
    pub since: Option<DateTime<Utc>>,
    // Pagination
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Extended listing view with entity name, tags, and provenance.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ListingDetail {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub entity_name: Option<String>,
    pub entity_type: Option<String>,
    pub source_url: Option<String>,
    pub location_text: Option<String>,
    pub schedule_description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub source_locale: String,
    pub locale: String,
    pub is_fallback: bool,
}

impl ListingDetail {
    /// Find active listings with default English locale (no translation joins).
    pub async fn find_active(limit: i64, offset: i64, pool: &PgPool) -> Result<Vec<Self>> {
        Self::find_active_localized(limit, offset, "en", pool).await
    }

    /// Find active listings with translated content for the given locale.
    /// Cluster-aware: returns one listing per cluster (best active member) + unclustered listings.
    /// Fallback chain: requested locale → English → source text.
    pub async fn find_active_localized(
        limit: i64,
        offset: i64,
        locale: &str,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            WITH cluster_reps AS (
                SELECT DISTINCT ON (c.id)
                    ci.item_id AS listing_id
                FROM clusters c
                JOIN cluster_items ci ON ci.cluster_id = c.id AND ci.item_type = 'listing'
                JOIN listings l ON l.id = ci.item_id
                WHERE c.cluster_type = 'listing'
                  AND l.status = 'active'
                ORDER BY c.id,
                    (ci.item_id = c.representative_id) DESC,
                    ci.similarity_score DESC NULLS LAST
            )
            SELECT
                l.id,
                COALESCE(t_title.content, en_title.content, l.title) as title,
                COALESCE(t_desc.content, en_desc.content, l.description) as description,
                l.status,
                e.name as entity_name, e.entity_type,
                l.source_url, l.location_text,
                COALESCE(s.description, to_char(s.valid_from, 'YYYY-MM-DD HH24:MI')) as schedule_description,
                l.created_at,
                l.source_locale,
                CASE
                    WHEN t_title.content IS NOT NULL THEN $1
                    WHEN en_title.content IS NOT NULL THEN 'en'
                    ELSE l.source_locale
                END as locale,
                CASE
                    WHEN t_title.content IS NOT NULL THEN false
                    ELSE true
                END as is_fallback
            FROM listings l
            LEFT JOIN entities e ON e.id = l.entity_id
            LEFT JOIN LATERAL (
                SELECT description, valid_from FROM schedules
                WHERE scheduleable_type = 'listing' AND scheduleable_id = l.id
                ORDER BY valid_from DESC NULLS LAST
                LIMIT 1
            ) s ON true
            LEFT JOIN translations t_title
                ON t_title.translatable_type = 'listing'
                AND t_title.translatable_id = l.id
                AND t_title.field_name = 'title'
                AND t_title.locale = $1
            LEFT JOIN translations t_desc
                ON t_desc.translatable_type = 'listing'
                AND t_desc.translatable_id = l.id
                AND t_desc.field_name = 'description'
                AND t_desc.locale = $1
            LEFT JOIN translations en_title
                ON en_title.translatable_type = 'listing'
                AND en_title.translatable_id = l.id
                AND en_title.field_name = 'title'
                AND en_title.locale = 'en'
            LEFT JOIN translations en_desc
                ON en_desc.translatable_type = 'listing'
                AND en_desc.translatable_id = l.id
                AND en_desc.field_name = 'description'
                AND en_desc.locale = 'en'
            WHERE l.status = 'active'
              AND (
                l.id IN (SELECT listing_id FROM cluster_reps)
                OR
                l.id NOT IN (SELECT item_id FROM cluster_items WHERE item_type = 'listing')
              )
            ORDER BY l.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(locale)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Find all listings in the same cluster as the given listing (for provenance/detail view).
    pub async fn cluster_siblings(listing_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT
                l.id, l.title, l.description, l.status,
                e.name as entity_name, e.entity_type,
                l.source_url, l.location_text,
                COALESCE(sch.description, to_char(sch.valid_from, 'YYYY-MM-DD HH24:MI')) as schedule_description,
                l.created_at,
                l.source_locale,
                l.source_locale as locale,
                false as is_fallback
            FROM cluster_items ci
            JOIN cluster_items ci2 ON ci2.cluster_id = ci.cluster_id AND ci2.item_type = 'listing'
            JOIN listings l ON l.id = ci2.item_id
            LEFT JOIN entities e ON e.id = l.entity_id
            LEFT JOIN LATERAL (
                SELECT description, valid_from FROM schedules
                WHERE scheduleable_type = 'listing' AND scheduleable_id = l.id
                ORDER BY valid_from DESC NULLS LAST
                LIMIT 1
            ) sch ON true
            WHERE ci.item_type = 'listing' AND ci.item_id = $1
              AND ci2.item_id != $1
            ORDER BY ci2.similarity_score DESC NULLS LAST
            "#,
        )
        .bind(listing_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}

/// Listing with distance from a reference zip code.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ListingWithDistance {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub entity_name: Option<String>,
    pub entity_type: Option<String>,
    pub source_url: Option<String>,
    pub location_text: Option<String>,
    pub schedule_description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub source_locale: String,
    pub locale: String,
    pub is_fallback: bool,
    pub distance_miles: f64,
    pub zip_code: Option<String>,
    pub location_city: Option<String>,
}

impl ListingWithDistance {
    /// Find listings near a zip code with haversine distance, translation support, and tag filters.
    pub async fn find_near_zip(
        zip: &str,
        radius_miles: f64,
        filters: &ListingFilters,
        limit: i64,
        offset: i64,
        locale: &str,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        let radius_miles = radius_miles.min(100.0);
        let lat_delta = radius_miles / 69.0;
        let lng_delta = radius_miles / 54.6; // ~69 * cos(45°) for MN latitude

        let mut qb = sqlx::QueryBuilder::new(
            r#"WITH center AS (
                SELECT latitude, longitude FROM zip_codes WHERE zip_code = "#,
        );
        qb.push_bind(zip);
        qb.push(
            r#")
            SELECT
                l.id,
                COALESCE(t_title.content, en_title.content, l.title) as title,
                COALESCE(t_desc.content, en_desc.content, l.description) as description,
                l.status,
                e.name as entity_name, e.entity_type,
                l.source_url, l.location_text,
                COALESCE(sch.description, to_char(sch.valid_from, 'YYYY-MM-DD HH24:MI')) as schedule_description,
                l.created_at,
                l.source_locale,
                CASE
                    WHEN t_title.content IS NOT NULL THEN "#,
        );
        qb.push_bind(locale);
        qb.push(
            r#"
                    WHEN en_title.content IS NOT NULL THEN 'en'
                    ELSE l.source_locale
                END as locale,
                CASE
                    WHEN t_title.content IS NOT NULL THEN false
                    ELSE true
                END as is_fallback,
                MIN(haversine_distance(center.latitude, center.longitude, loc.latitude, loc.longitude)) as distance_miles,
                loc.postal_code as zip_code,
                loc.city as location_city
            FROM listings l
            CROSS JOIN center
            LEFT JOIN LATERAL (
                SELECT description, valid_from FROM schedules
                WHERE scheduleable_type = 'listing' AND scheduleable_id = l.id
                ORDER BY valid_from DESC NULLS LAST
                LIMIT 1
            ) sch ON true
            JOIN locationables la ON la.locatable_type = 'listing' AND la.locatable_id = l.id
            JOIN locations loc ON loc.id = la.location_id
            LEFT JOIN entities e ON e.id = l.entity_id
            LEFT JOIN translations t_title
                ON t_title.translatable_type = 'listing'
                AND t_title.translatable_id = l.id
                AND t_title.field_name = 'title'
                AND t_title.locale = "#,
        );
        qb.push_bind(locale);
        qb.push(
            r#"
            LEFT JOIN translations t_desc
                ON t_desc.translatable_type = 'listing'
                AND t_desc.translatable_id = l.id
                AND t_desc.field_name = 'description'
                AND t_desc.locale = "#,
        );
        qb.push_bind(locale);
        qb.push(
            r#"
            LEFT JOIN translations en_title
                ON en_title.translatable_type = 'listing'
                AND en_title.translatable_id = l.id
                AND en_title.field_name = 'title'
                AND en_title.locale = 'en'
            LEFT JOIN translations en_desc
                ON en_desc.translatable_type = 'listing'
                AND en_desc.translatable_id = l.id
                AND en_desc.field_name = 'description'
                AND en_desc.locale = 'en'
            WHERE l.status = 'active'
              AND (l.expires_at IS NULL OR l.expires_at > NOW())
              AND loc.latitude IS NOT NULL
              AND loc.latitude BETWEEN center.latitude - "#,
        );
        qb.push_bind(lat_delta);
        qb.push(" AND center.latitude + ");
        qb.push_bind(lat_delta);
        qb.push(" AND loc.longitude BETWEEN center.longitude - ");
        qb.push_bind(lng_delta);
        qb.push(" AND center.longitude + ");
        qb.push_bind(lng_delta);
        qb.push(" ");

        // Tag filters
        let tag_filters: Vec<(&str, &Option<String>)> = vec![
            ("signal_domain", &filters.signal_domain),
            ("audience_role", &filters.audience_role),
            ("category", &filters.category),
            ("listing_type", &filters.listing_type),
            ("urgency", &filters.urgency),
            ("confidence", &filters.confidence),
            ("capacity_status", &filters.capacity_status),
            ("radius_relevant", &filters.radius_relevant),
            ("population", &filters.population),
        ];

        for (kind, value) in &tag_filters {
            if let Some(val) = value {
                qb.push("AND EXISTS (SELECT 1 FROM taggables tg JOIN tags t ON t.id = tg.tag_id WHERE tg.taggable_type = 'listing' AND tg.taggable_id = l.id AND t.kind = ");
                qb.push_bind(*kind);
                qb.push(" AND t.value = ");
                qb.push_bind(val.clone());
                qb.push(") ");
            }
        }

        qb.push(
            "GROUP BY l.id, l.title, l.description, l.status, e.name, e.entity_type, \
             l.source_url, l.location_text, sch.description, sch.valid_from, l.created_at, \
             l.source_locale, t_title.content, t_desc.content, en_title.content, en_desc.content, \
             loc.postal_code, loc.city \
             HAVING MIN(haversine_distance(center.latitude, center.longitude, loc.latitude, loc.longitude)) <= ",
        );
        qb.push_bind(radius_miles);
        qb.push(" ORDER BY distance_miles ASC ");
        qb.push("LIMIT ");
        qb.push_bind(limit);
        qb.push(" OFFSET ");
        qb.push_bind(offset);

        qb.build_query_as::<Self>()
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    /// Count listings near a zip code within a radius.
    pub async fn count_near_zip(
        zip: &str,
        radius_miles: f64,
        filters: &ListingFilters,
        pool: &PgPool,
    ) -> Result<i64> {
        let radius_miles = radius_miles.min(100.0);
        let lat_delta = radius_miles / 69.0;
        let lng_delta = radius_miles / 54.6;

        let mut qb = sqlx::QueryBuilder::new(
            r#"WITH center AS (
                SELECT latitude, longitude FROM zip_codes WHERE zip_code = "#,
        );
        qb.push_bind(zip);
        qb.push(
            r#")
            SELECT COUNT(DISTINCT l.id)
            FROM listings l
            CROSS JOIN center
            JOIN locationables la ON la.locatable_type = 'listing' AND la.locatable_id = l.id
            JOIN locations loc ON loc.id = la.location_id
            WHERE l.status = 'active'
              AND (l.expires_at IS NULL OR l.expires_at > NOW())
              AND loc.latitude IS NOT NULL
              AND loc.latitude BETWEEN center.latitude - "#,
        );
        qb.push_bind(lat_delta);
        qb.push(" AND center.latitude + ");
        qb.push_bind(lat_delta);
        qb.push(" AND loc.longitude BETWEEN center.longitude - ");
        qb.push_bind(lng_delta);
        qb.push(" AND center.longitude + ");
        qb.push_bind(lng_delta);
        qb.push(" AND haversine_distance(center.latitude, center.longitude, loc.latitude, loc.longitude) <= ");
        qb.push_bind(radius_miles);
        qb.push(" ");

        let tag_filters: Vec<(&str, &Option<String>)> = vec![
            ("signal_domain", &filters.signal_domain),
            ("audience_role", &filters.audience_role),
            ("category", &filters.category),
            ("listing_type", &filters.listing_type),
            ("urgency", &filters.urgency),
            ("confidence", &filters.confidence),
            ("capacity_status", &filters.capacity_status),
            ("radius_relevant", &filters.radius_relevant),
            ("population", &filters.population),
        ];

        for (kind, value) in &tag_filters {
            if let Some(val) = value {
                qb.push("AND EXISTS (SELECT 1 FROM taggables tg JOIN tags t ON t.id = tg.tag_id WHERE tg.taggable_type = 'listing' AND tg.taggable_id = l.id AND t.kind = ");
                qb.push_bind(*kind);
                qb.push(" AND t.value = ");
                qb.push_bind(val.clone());
                qb.push(") ");
            }
        }

        let row = qb
            .build_query_as::<(i64,)>()
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }
}

/// Stats for the assessment view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingStats {
    pub total_listings: i64,
    pub active_listings: i64,
    pub total_sources: i64,
    pub total_snapshots: i64,
    pub total_extractions: i64,
    pub total_entities: i64,
    pub listings_by_type: Vec<TagCount>,
    pub listings_by_role: Vec<TagCount>,
    pub listings_by_category: Vec<TagCount>,
    pub listings_by_domain: Vec<TagCount>,
    pub listings_by_urgency: Vec<TagCount>,
    pub listings_by_confidence: Vec<TagCount>,
    pub listings_by_capacity: Vec<TagCount>,
    pub recent_7d: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TagCount {
    pub value: String,
    pub count: i64,
}

impl ListingStats {
    pub async fn compute(pool: &PgPool) -> Result<Self> {
        let total_listings = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM listings")
            .fetch_one(pool)
            .await?
            .0;

        let active_listings =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM listings WHERE status = 'active'")
                .fetch_one(pool)
                .await?
                .0;

        let total_sources = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM sources")
            .fetch_one(pool)
            .await?
            .0;

        let total_snapshots = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM page_snapshots")
            .fetch_one(pool)
            .await?
            .0;

        let total_extractions = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM extractions")
            .fetch_one(pool)
            .await?
            .0;

        let total_entities = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM entities")
            .fetch_one(pool)
            .await?
            .0;

        let recent_7d = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(DISTINCT s.scheduleable_id) FROM schedules s JOIN listings l ON l.id = s.scheduleable_id WHERE s.scheduleable_type = 'listing' AND s.valid_from > NOW() - INTERVAL '7 days'",
        )
        .fetch_one(pool)
        .await?
        .0;

        let listings_by_type = sqlx::query_as::<_, TagCount>(
            r#"
            SELECT t.value, COUNT(*) as count
            FROM taggables tb
            JOIN tags t ON t.id = tb.tag_id
            WHERE t.kind = 'listing_type' AND tb.taggable_type = 'listing'
            GROUP BY t.value
            ORDER BY count DESC
            "#,
        )
        .fetch_all(pool)
        .await?;

        let listings_by_role = sqlx::query_as::<_, TagCount>(
            r#"
            SELECT t.value, COUNT(*) as count
            FROM taggables tb
            JOIN tags t ON t.id = tb.tag_id
            WHERE t.kind = 'audience_role' AND tb.taggable_type = 'listing'
            GROUP BY t.value
            ORDER BY count DESC
            "#,
        )
        .fetch_all(pool)
        .await?;

        let listings_by_category = Self::count_by_tag_kind("category", pool).await?;
        let listings_by_domain = Self::count_by_tag_kind("signal_domain", pool).await?;
        let listings_by_urgency = Self::count_by_tag_kind("urgency", pool).await?;
        let listings_by_confidence = Self::count_by_tag_kind("confidence", pool).await?;
        let listings_by_capacity = Self::count_by_tag_kind("capacity_status", pool).await?;

        Ok(Self {
            total_listings,
            active_listings,
            total_sources,
            total_snapshots,
            total_extractions,
            total_entities,
            listings_by_type,
            listings_by_role,
            listings_by_category,
            listings_by_domain,
            listings_by_urgency,
            listings_by_confidence,
            listings_by_capacity,
            recent_7d,
        })
    }

    pub async fn count_by_tag_kind_public(kind: &str, pool: &PgPool) -> Result<Vec<TagCount>> {
        Self::count_by_tag_kind(kind, pool).await
    }

    async fn count_by_tag_kind(kind: &str, pool: &PgPool) -> Result<Vec<TagCount>> {
        sqlx::query_as::<_, TagCount>(
            r#"
            SELECT t.value, COUNT(*) as count
            FROM taggables tb
            JOIN tags t ON t.id = tb.tag_id
            WHERE t.kind = $1 AND tb.taggable_type = 'listing'
            GROUP BY t.value
            ORDER BY count DESC
            "#,
        )
        .bind(kind)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
