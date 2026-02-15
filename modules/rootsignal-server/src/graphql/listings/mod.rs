pub mod mutations;
pub mod types;

use async_graphql::connection::*;
use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::graphql::context::Locale;
use crate::graphql::error;
use types::{GqlListing, GqlListingEdgeData};

#[derive(Default)]
pub struct ListingQuery;

#[Object]
impl ListingQuery {
    /// Fetch a single listing by ID.
    async fn listing(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlListing> {
        tracing::info!(id = %id, "graphql.listing");
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let listing = rootsignal_domains::listings::Listing::find_by_id(id, pool)
            .await
            .map_err(|_| error::not_found(format!("listing {id}")))?;
        Ok(GqlListing::from(listing))
    }

    /// Paginated listing connection with optional filters.
    /// When `zipCode` and `radiusMiles` are provided, results are sorted by distance
    /// and edge data includes `distanceMiles`, `zipCode`, and `locationCity`.
    #[graphql(complexity = "first.unwrap_or(20) as usize * child_complexity + 1")]
    async fn listings(
        &self,
        ctx: &Context<'_>,
        after: Option<String>,
        first: Option<i32>,
        // Filters
        signal_domain: Option<String>,
        audience_role: Option<String>,
        category: Option<String>,
        listing_type: Option<String>,
        urgency: Option<String>,
        confidence: Option<String>,
        capacity_status: Option<String>,
        // Geo
        zip_code: Option<String>,
        radius_miles: Option<f64>,
        // Temporal
        since: Option<DateTime<Utc>>,
    ) -> Result<Connection<String, GqlListing, EmptyFields, GqlListingEdgeData>> {
        tracing::info!(first = ?first, zip_code = ?zip_code, "graphql.listings");
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let _locale = ctx.data_unchecked::<Locale>();

        let tag_filters: Vec<(&str, Option<String>)> = vec![
            ("signal_domain", signal_domain),
            ("audience_role", audience_role),
            ("category", category),
            ("listing_type", listing_type),
            ("urgency", urgency),
            ("confidence", confidence),
            ("capacity_status", capacity_status),
        ];

        if let Some(zip) = zip_code {
            // Geo path: query with haversine distance, sorted by distance
            let radius = radius_miles.unwrap_or(25.0).min(100.0);
            query_geo(pool, zip, radius, &tag_filters, since, after, first).await
        } else {
            // Standard path: keyset pagination by (created_at, id)
            query_standard(pool, &tag_filters, since, after, first).await
        }
    }
}

/// Standard listing query with keyset pagination by (created_at, id).
async fn query_standard(
    pool: &sqlx::PgPool,
    tag_filters: &[(&str, Option<String>)],
    since: Option<DateTime<Utc>>,
    after: Option<String>,
    first: Option<i32>,
) -> Result<Connection<String, GqlListing, EmptyFields, GqlListingEdgeData>> {
    query(
        after,
        None::<String>,
        first,
        None::<i32>,
        |after: Option<String>, _before, first, _last| async move {
            let limit = first.unwrap_or(20).min(100) as i64;

            let (after_created_at, after_id) = if let Some(cursor) = &after {
                decode_cursor(cursor)?
            } else {
                (None, None)
            };

            let mut qb = sqlx::QueryBuilder::new(
                "SELECT * FROM listings WHERE status = 'active' AND (expires_at IS NULL OR expires_at > NOW()) ",
            );

            if let (Some(ca), Some(ai)) = (&after_created_at, &after_id) {
                qb.push("AND (created_at, id) < (");
                qb.push_bind(*ca);
                qb.push(", ");
                qb.push_bind(*ai);
                qb.push(") ");
            }

            push_tag_filters(&mut qb, tag_filters, "listings");
            push_temporal_filter(&mut qb, &since);

            qb.push("ORDER BY created_at DESC, id DESC ");
            qb.push("LIMIT ");
            qb.push_bind(limit + 1);

            let rows = qb
                .build_query_as::<rootsignal_domains::listings::Listing>()
                .fetch_all(pool)
                .await
                .map_err(|e| error::internal(e))?;

            let has_next = rows.len() as i64 > limit;
            let has_prev = after.is_some();
            let nodes: Vec<_> = rows.into_iter().take(limit as usize).collect();

            let mut connection =
                Connection::with_additional_fields(has_prev, has_next, EmptyFields);
            connection.edges.extend(nodes.into_iter().map(|n| {
                let cursor = encode_cursor(&n.created_at, &n.id);
                Edge::with_additional_fields(
                    cursor,
                    GqlListing::from(n),
                    GqlListingEdgeData::empty(),
                )
            }));

            Ok::<_, async_graphql::Error>(connection)
        },
    )
    .await
}

/// Row returned by the geo query — raw listing fields plus distance metadata.
#[derive(sqlx::FromRow)]
struct ListingWithGeo {
    // Listing fields
    id: Uuid,
    title: String,
    description: Option<String>,
    status: String,
    entity_id: Option<Uuid>,
    service_id: Option<Uuid>,
    source_url: Option<String>,
    location_text: Option<String>,
    in_language: String,
    expires_at: Option<DateTime<Utc>>,
    freshness_score: f32,
    relevance_score: Option<i32>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    // Geo fields
    distance_miles: f64,
    nearest_zip: Option<String>,
    nearest_city: Option<String>,
}

impl From<ListingWithGeo> for (GqlListing, GqlListingEdgeData) {
    fn from(r: ListingWithGeo) -> Self {
        let listing = GqlListing {
            id: r.id,
            title: r.title,
            description: r.description,
            status: r.status,
            entity_id: r.entity_id,
            service_id: r.service_id,
            source_url: r.source_url,
            location_text: r.location_text,
            in_language: r.in_language,
            expires_at: r.expires_at,
            freshness_score: r.freshness_score,
            relevance_score: r.relevance_score,
            created_at: r.created_at,
            updated_at: r.updated_at,
        };
        let edge = GqlListingEdgeData {
            distance_miles: Some(r.distance_miles),
            zip_code: r.nearest_zip,
            location_city: r.nearest_city,
        };
        (listing, edge)
    }
}

/// Geo listing query with haversine distance, sorted by distance.
async fn query_geo(
    pool: &sqlx::PgPool,
    zip: String,
    radius: f64,
    tag_filters: &[(&str, Option<String>)],
    since: Option<DateTime<Utc>>,
    after: Option<String>,
    first: Option<i32>,
) -> Result<Connection<String, GqlListing, EmptyFields, GqlListingEdgeData>> {
    query(
        after,
        None::<String>,
        first,
        None::<i32>,
        |after: Option<String>, _before, first, _last| async move {
            let limit = first.unwrap_or(20).min(100) as i64;
            let lat_delta = radius / 69.0;

            // Decode geo cursor: "distance|uuid"
            let (after_distance, after_id) = if let Some(cursor) = &after {
                decode_geo_cursor(cursor)?
            } else {
                (None, None)
            };

            let mut qb = sqlx::QueryBuilder::new(
                "WITH center AS (SELECT latitude, longitude FROM zip_codes WHERE zip_code = ",
            );
            qb.push_bind(&zip);
            qb.push(
                r#")
                SELECT
                    l.id, l.title, l.description, l.status,
                    l.entity_id, l.service_id, l.source_url, l.location_text,
                    l.in_language, l.expires_at, l.freshness_score, l.relevance_score,
                    l.created_at, l.updated_at,
                    MIN(haversine_distance(center.latitude, center.longitude, loc.latitude, loc.longitude)) as distance_miles,
                    loc.postal_code as nearest_zip,
                    loc.address_locality as nearest_city
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
            qb.push(" AND loc.longitude BETWEEN center.longitude - (");
            qb.push_bind(radius);
            qb.push(" / (69.0 * cos(radians(center.latitude)))) AND center.longitude + (");
            qb.push_bind(radius);
            qb.push(" / (69.0 * cos(radians(center.latitude)))) ");

            push_tag_filters(&mut qb, tag_filters, "l");
            push_temporal_filter_prefixed(&mut qb, &since, "l.");

            qb.push(
                "GROUP BY l.id, l.title, l.description, l.status, l.entity_id, l.service_id, \
                 l.source_url, l.location_text, l.in_language, l.expires_at, l.freshness_score, \
                 l.relevance_score, l.created_at, l.updated_at, loc.postal_code, loc.address_locality ",
            );
            qb.push("HAVING MIN(haversine_distance(center.latitude, center.longitude, loc.latitude, loc.longitude)) <= ");
            qb.push_bind(radius);
            qb.push(" ");

            // Geo cursor condition (filter after grouping)
            if let (Some(dist), Some(aid)) = (&after_distance, &after_id) {
                qb.push("AND (MIN(haversine_distance(center.latitude, center.longitude, loc.latitude, loc.longitude)), l.id) > (");
                qb.push_bind(*dist);
                qb.push(", ");
                qb.push_bind(*aid);
                qb.push(") ");
            }

            qb.push("ORDER BY distance_miles ASC, l.id ASC ");
            qb.push("LIMIT ");
            qb.push_bind(limit + 1);

            let rows = qb
                .build_query_as::<ListingWithGeo>()
                .fetch_all(pool)
                .await
                .map_err(|e| error::internal(e))?;

            let has_next = rows.len() as i64 > limit;
            let has_prev = after.is_some();
            let nodes: Vec<_> = rows.into_iter().take(limit as usize).collect();

            let mut connection =
                Connection::with_additional_fields(has_prev, has_next, EmptyFields);
            connection.edges.extend(nodes.into_iter().map(|r| {
                let (listing, edge) = <(GqlListing, GqlListingEdgeData)>::from(r);
                let cursor = encode_geo_cursor(edge.distance_miles.unwrap_or(0.0), &listing.id);
                Edge::with_additional_fields(cursor, listing, edge)
            }));

            Ok::<_, async_graphql::Error>(connection)
        },
    )
    .await
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn push_tag_filters<'a>(
    qb: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    filters: &[(&'a str, Option<String>)],
    table_alias: &str,
) {
    for (kind, value) in filters {
        if let Some(val) = value {
            qb.push(format!(
                "AND EXISTS (SELECT 1 FROM taggables tg JOIN tags t ON t.id = tg.tag_id \
                 WHERE tg.taggable_type = 'listing' AND tg.taggable_id = {table_alias}.id AND t.kind = "
            ));
            qb.push_bind(*kind);
            qb.push(" AND t.value = ");
            qb.push_bind(val.clone());
            qb.push(") ");
        }
    }
}

fn push_temporal_filter(
    qb: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    since: &Option<DateTime<Utc>>,
) {
    if let Some(since_dt) = since {
        qb.push("AND created_at >= ");
        qb.push_bind(*since_dt);
        qb.push(" ");
    }
}

fn push_temporal_filter_prefixed(
    qb: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    since: &Option<DateTime<Utc>>,
    prefix: &str,
) {
    if let Some(since_dt) = since {
        qb.push(format!("AND {prefix}created_at >= "));
        qb.push_bind(*since_dt);
        qb.push(" ");
    }
}

// ── Cursors ──────────────────────────────────────────────────────────────────

/// Encode a standard cursor: base64("created_at_rfc3339|uuid").
fn encode_cursor(created_at: &DateTime<Utc>, id: &Uuid) -> String {
    use base64::Engine;
    let raw = format!("{}|{}", created_at.to_rfc3339(), id);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

/// Decode a standard cursor.
fn decode_cursor(cursor: &str) -> Result<(Option<DateTime<Utc>>, Option<Uuid>)> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| error::bad_request("invalid cursor"))?;
    let s =
        String::from_utf8(decoded).map_err(|_| error::bad_request("invalid cursor encoding"))?;
    let parts: Vec<&str> = s.splitn(2, '|').collect();
    if parts.len() != 2 {
        return Err(error::bad_request("malformed cursor"));
    }
    let dt = DateTime::parse_from_rfc3339(parts[0])
        .map_err(|_| error::bad_request("invalid cursor timestamp"))?
        .with_timezone(&Utc);
    let id = parts[1]
        .parse::<Uuid>()
        .map_err(|_| error::bad_request("invalid cursor id"))?;
    Ok((Some(dt), Some(id)))
}

/// Encode a geo cursor: base64("distance|uuid").
fn encode_geo_cursor(distance: f64, id: &Uuid) -> String {
    use base64::Engine;
    let raw = format!("{}|{}", distance, id);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

/// Decode a geo cursor.
fn decode_geo_cursor(cursor: &str) -> Result<(Option<f64>, Option<Uuid>)> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| error::bad_request("invalid cursor"))?;
    let s =
        String::from_utf8(decoded).map_err(|_| error::bad_request("invalid cursor encoding"))?;
    let parts: Vec<&str> = s.splitn(2, '|').collect();
    if parts.len() != 2 {
        return Err(error::bad_request("malformed cursor"));
    }
    let distance: f64 = parts[0]
        .parse()
        .map_err(|_| error::bad_request("invalid cursor distance"))?;
    let id = parts[1]
        .parse::<Uuid>()
        .map_err(|_| error::bad_request("invalid cursor id"))?;
    Ok((Some(distance), Some(id)))
}
