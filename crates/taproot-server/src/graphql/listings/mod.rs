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
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let listing = taproot_domains::listings::Listing::find_by_id(id, pool)
            .await
            .map_err(|_| error::not_found(format!("listing {id}")))?;
        Ok(GqlListing::from(listing))
    }

    /// Paginated listing connection with optional filters.
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
        _radius_miles: Option<f64>,
        // Temporal
        since: Option<DateTime<Utc>>,
    ) -> Result<Connection<String, GqlListing, EmptyFields, GqlListingEdgeData>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let _locale = ctx.data_unchecked::<Locale>();

        query(after, None::<String>, first, None::<i32>, |after: Option<String>, _before, first, _last| async move {
            let limit = first.unwrap_or(20).min(100) as i64;

            // Decode cursor: "created_at_rfc3339|uuid"
            let (after_created_at, after_id) = if let Some(cursor) = &after {
                decode_cursor(cursor)?
            } else {
                (None, None)
            };

            let _has_filters = signal_domain.is_some()
                || audience_role.is_some()
                || category.is_some()
                || listing_type.is_some()
                || urgency.is_some()
                || confidence.is_some()
                || capacity_status.is_some()
                || zip_code.is_some()
                || since.is_some();

            // Build query using keyset pagination
            let mut qb = sqlx::QueryBuilder::new(
                "SELECT * FROM listings WHERE status = 'active' AND (expires_at IS NULL OR expires_at > NOW()) "
            );

            // Keyset cursor condition
            if let (Some(ca), Some(ai)) = (&after_created_at, &after_id) {
                qb.push("AND (created_at, id) < (");
                qb.push_bind(*ca);
                qb.push(", ");
                qb.push_bind(*ai);
                qb.push(") ");
            }

            // Tag filters
            let tag_filters: Vec<(&str, &Option<String>)> = vec![
                ("signal_domain", &signal_domain),
                ("audience_role", &audience_role),
                ("category", &category),
                ("listing_type", &listing_type),
                ("urgency", &urgency),
                ("confidence", &confidence),
                ("capacity_status", &capacity_status),
            ];

            for (kind, value) in &tag_filters {
                if let Some(val) = value {
                    qb.push("AND EXISTS (SELECT 1 FROM taggables tg JOIN tags t ON t.id = tg.tag_id WHERE tg.taggable_type = 'listing' AND tg.taggable_id = listings.id AND t.kind = ");
                    qb.push_bind(*kind);
                    qb.push(" AND t.value = ");
                    qb.push_bind(val.clone());
                    qb.push(") ");
                }
            }

            // Temporal
            if let Some(since_dt) = &since {
                qb.push("AND created_at >= ");
                qb.push_bind(*since_dt);
                qb.push(" ");
            }

            qb.push("ORDER BY created_at DESC, id DESC ");
            qb.push("LIMIT ");
            qb.push_bind(limit + 1); // fetch one extra to detect hasNextPage

            let rows = qb
                .build_query_as::<taproot_domains::listings::Listing>()
                .fetch_all(pool)
                .await
                .map_err(|e| error::internal(e))?;

            let has_next = rows.len() as i64 > limit;
            let has_prev = after.is_some();
            let nodes: Vec<_> = rows.into_iter().take(limit as usize).collect();

            let mut connection = Connection::with_additional_fields(has_prev, has_next, EmptyFields);
            connection.edges.extend(nodes.into_iter().map(|n| {
                let cursor = encode_cursor(&n.created_at, &n.id);
                Edge::with_additional_fields(cursor, GqlListing::from(n), GqlListingEdgeData {
                    distance_miles: None,
                    zip_code: None,
                    location_city: None,
                })
            }));

            Ok::<_, async_graphql::Error>(connection)
        })
        .await
    }
}

fn encode_cursor(created_at: &DateTime<Utc>, id: &Uuid) -> String {
    use base64::Engine;
    let raw = format!("{}|{}", created_at.to_rfc3339(), id);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

fn decode_cursor(cursor: &str) -> Result<(Option<DateTime<Utc>>, Option<Uuid>)> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| error::bad_request("invalid cursor"))?;
    let s = String::from_utf8(decoded)
        .map_err(|_| error::bad_request("invalid cursor encoding"))?;
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
