use anyhow::Result;
use taproot_core::{ExtractedListing, ServerDeps};
use uuid::Uuid;

use crate::entities::{Contact, Entity, Location, Locationable, Notable, Organization, Service, Taggable};

/// Extraction row with the data we need for normalization.
#[derive(Debug, sqlx::FromRow)]
struct ExtractionData {
    id: Uuid,
    page_snapshot_id: Uuid,
    data: serde_json::Value,
    fingerprint: Vec<u8>,
}

/// Normalize an extraction into entities, listings, tags, etc.
pub async fn normalize_extraction(
    extraction_id: Uuid,
    source_id: Option<Uuid>,
    deps: &ServerDeps,
) -> Result<Option<Uuid>> {
    let pool = deps.pool();

    let extraction = sqlx::query_as::<_, ExtractionData>(
        "SELECT id, page_snapshot_id, data, fingerprint FROM extractions WHERE id = $1",
    )
    .bind(extraction_id)
    .fetch_one(pool)
    .await?;

    let listing_data: ExtractedListing = serde_json::from_value(extraction.data)?;

    // Check for duplicate by fingerprint
    let fingerprint_hex = hex::encode(&extraction.fingerprint);
    let existing = sqlx::query_as::<_, (Uuid,)>(
        "SELECT listing_id FROM listing_extractions WHERE fingerprint = $1",
    )
    .bind(&fingerprint_hex)
    .fetch_optional(pool)
    .await?;

    if let Some((existing_listing_id,)) = existing {
        // Already normalized — just link this extraction to existing listing
        sqlx::query(
            r#"
            INSERT INTO listing_extractions (listing_id, extraction_id, page_snapshot_id, fingerprint, source_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(existing_listing_id)
        .bind(extraction_id)
        .bind(extraction.page_snapshot_id)
        .bind(&fingerprint_hex)
        .bind(source_id)
        .execute(pool)
        .await?;

        tracing::debug!(extraction_id = %extraction_id, listing_id = %existing_listing_id, "Linked to existing listing (dedup)");
        return Ok(Some(existing_listing_id));
    }

    // ── Find or create entity ───────────────────────────────────────────────

    let entity_id = if let Some(org_name) = &listing_data.organization_name {
        let entity_type = listing_data
            .organization_type
            .as_deref()
            .unwrap_or("organization");
        let entity = Entity::find_or_create(org_name, entity_type, None, None, pool).await?;

        // Create type-specific record if it doesn't exist
        if entity_type == "organization" || entity_type == "nonprofit" || entity_type == "community" || entity_type == "faith" || entity_type == "coalition" {
            if Organization::find_by_entity_id(entity.id, pool).await?.is_none() {
                let _ = Organization::create(
                    entity.id,
                    Some(entity_type),
                    None,
                    pool,
                ).await;
            }
        }

        Some(entity.id)
    } else {
        None
    };

    // ── Find or create service ──────────────────────────────────────────────

    let service_id = match (&entity_id, &listing_data.service_name) {
        (Some(eid), Some(sname)) => {
            let svc = Service::find_or_create(*eid, sname, None, pool).await?;
            Some(svc.id)
        }
        _ => None,
    };

    // ── Compute expires_at ──────────────────────────────────────────────────

    let source_locale = listing_data.source_locale.as_deref().unwrap_or("en");

    let timing_start = listing_data
        .start_time
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let timing_end = listing_data
        .end_time
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let expires_at = listing_data
        .expires_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or(timing_end);

    // ── Create listing ──────────────────────────────────────────────────────

    let listing = sqlx::query_as::<_, (Uuid,)>(
        r#"
        INSERT INTO listings (title, description, entity_id, service_id, source_url, location_text, timing_start, timing_end, source_locale, expires_at, freshness_score)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 1.0)
        RETURNING id
        "#,
    )
    .bind(&listing_data.title)
    .bind(&listing_data.description)
    .bind(entity_id)
    .bind(service_id)
    .bind(&listing_data.source_url)
    .bind(&listing_data.location_text)
    .bind(timing_start)
    .bind(timing_end)
    .bind(source_locale)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    let listing_id = listing.0;

    // ── Create listing_extraction provenance ─────────────────────────────────

    sqlx::query(
        r#"
        INSERT INTO listing_extractions (listing_id, extraction_id, page_snapshot_id, fingerprint, source_id, extraction_confidence)
        VALUES ($1, $2, $3, $4, $5, 'medium')
        "#,
    )
    .bind(listing_id)
    .bind(extraction_id)
    .bind(extraction.page_snapshot_id)
    .bind(&fingerprint_hex)
    .bind(source_id)
    .execute(pool)
    .await?;

    // ── Tags — all taxonomy dimensions via Taggable::tag() ──────────────────

    Taggable::tag("listing", listing_id, "listing_type", &listing_data.listing_type, pool).await?;

    for cat in &listing_data.categories {
        Taggable::tag("listing", listing_id, "category", cat, pool).await?;
    }

    for role in &listing_data.audience_roles {
        Taggable::tag("listing", listing_id, "audience_role", role, pool).await?;
    }

    if let Some(domain) = &listing_data.signal_domain {
        Taggable::tag("listing", listing_id, "signal_domain", domain, pool).await?;
    }

    if let Some(urgency) = &listing_data.urgency {
        Taggable::tag("listing", listing_id, "urgency", urgency, pool).await?;
    }

    if let Some(confidence) = &listing_data.confidence_hint {
        Taggable::tag("listing", listing_id, "confidence", confidence, pool).await?;
    }

    if let Some(capacity) = &listing_data.capacity_status {
        Taggable::tag("listing", listing_id, "capacity_status", capacity, pool).await?;
    }

    if let Some(radius) = &listing_data.radius_relevant {
        Taggable::tag("listing", listing_id, "radius_relevant", radius, pool).await?;
    }

    if let Some(populations) = &listing_data.populations {
        for pop in populations {
            Taggable::tag("listing", listing_id, "population", pop, pool).await?;
        }
    }

    // ── Location ────────────────────────────────────────────────────────────

    if listing_data.address.is_some() || listing_data.city.is_some() {
        let location = Location::find_or_create_from_extraction(
            listing_data.city.as_deref(),
            listing_data.state.as_deref(),
            listing_data.postal_code.as_deref(),
            listing_data.address.as_deref(),
            pool,
        )
        .await?;

        Locationable::create(location.id, "listing", listing_id, true, pool).await?;
    }

    // ── Contact ─────────────────────────────────────────────────────────────

    if listing_data.contact_email.is_some() || listing_data.contact_phone.is_some() {
        Contact::create(
            "listing",
            listing_id,
            listing_data.contact_name.as_deref(),
            listing_data.contact_email.as_deref(),
            listing_data.contact_phone.as_deref(),
            pool,
        )
        .await?;
    }

    // ── Capacity notes (keep as notes for freeform text) ────────────────────

    if let Some(capacity_note) = &listing_data.capacity_note {
        Notable::attach_note(
            "listing",
            listing_id,
            capacity_note,
            "warning",
            Some("ai_extraction"),
            "ai",
            pool,
        )
        .await?;
    }

    tracing::info!(listing_id = %listing_id, title = %listing_data.title, "Normalized listing");
    Ok(Some(listing_id))
}
