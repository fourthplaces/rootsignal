use tracing::{info, warn};
use uuid::Uuid;

use ai_client::claude::Claude;
use rootsignal_archive::router::{detect_target, TargetKind};
use rootsignal_archive::{extract_links_by_pattern, Archive};
use rootsignal_common::{
    ActorNode, ActorType, DiscoveryMethod, SourceNode, SourceRole,
};
use rootsignal_graph::GraphWriter;

/// LLM extraction schema for actor identity from a web page.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ActorPageExtraction {
    /// Whether this page represents or is primarily about a specific actor
    is_actor_page: bool,
    /// Name of the organization, group, or individual (if is_actor_page)
    name: Option<String>,
    /// One of: "organization", "government_body", "coalition", "individual"
    actor_type: Option<String>,
    /// Short bio/description of what this actor does
    bio: Option<String>,
    /// Location name if mentioned (city, neighborhood, region)
    location: Option<String>,
}

#[derive(serde::Deserialize)]
struct NominatimResult {
    lat: String,
    lon: String,
    display_name: String,
}

/// Result of discovering an actor from a page (no GraphQL dependency).
pub struct ActorDiscoveryResult {
    pub actor_id: Uuid,
    pub location_name: String,
}

pub async fn geocode_location(location: &str) -> anyhow::Result<(f64, f64, String)> {
    if location.len() > 200 {
        anyhow::bail!("Location input too long (max 200 chars)");
    }
    let client = reqwest::Client::new();
    let resp = client
        .get("https://nominatim.openstreetmap.org/search")
        .query(&[("q", location), ("format", "json"), ("limit", "1")])
        .header("User-Agent", "rootsignal/1.0")
        .send()
        .await?;

    let results: Vec<NominatimResult> = resp.json().await?;
    let first = results
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No geocoding results for '{}'", location))?;

    let lat: f64 = first.lat.parse()?;
    let lon: f64 = first.lon.parse()?;
    Ok((lat, lon, first.display_name))
}

/// Create an actor from a web page.
///
/// 1. Fetch the page
/// 2. Scan HTML for social links (deterministic)
/// 3. Optionally gate on social links (discover mode skips pages without them)
/// 4. LLM extraction to determine if page represents an actor
/// 5. Geocode, deduplicate, create actor + source nodes
pub async fn create_actor_from_page(
    archive: &Archive,
    writer: &GraphWriter,
    anthropic_api_key: &str,
    url: &str,
    fallback_region: &str,
    require_social_links: bool,
) -> anyhow::Result<Option<ActorDiscoveryResult>> {
    // 1. Fetch the page
    let page = archive.page(url).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    // 2. Scan HTML for social links
    let all_links = extract_links_by_pattern(&page.raw_html, url, "");
    let mut social_links: Vec<(rootsignal_common::SocialPlatform, String, String)> = Vec::new();
    for link in &all_links {
        if let TargetKind::Social {
            platform,
            identifier,
        } = detect_target(link)
        {
            social_links.push((platform, identifier, link.clone()));
        }
    }

    // 3. Gate on social links for discover mode
    if require_social_links && social_links.is_empty() {
        return Ok(None);
    }

    // 4. LLM extraction
    let claude = Claude::new(anthropic_api_key, "claude-haiku-4-5-20251001");
    let system = "You analyze web pages to determine if they represent a specific actor \
        (organization, group, government body, or individual). \
        Set is_actor_page to true ONLY if the page IS about a single identifiable actor \
        (e.g. org homepage, Linktree, about page). \
        Set is_actor_page to false if the page merely mentions actors \
        (e.g. news article, directory listing, search results).";

    let extraction: ActorPageExtraction = claude
        .extract("claude-haiku-4-5-20251001", system, &page.markdown)
        .await?;

    if !extraction.is_actor_page {
        return Ok(None);
    }
    let name = match extraction.name {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => return Ok(None),
    };

    // 5. Geocode location
    let location_str = extraction
        .location
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| fallback_region.to_string());
    let (lat, lng, display_name) = geocode_location(&location_str).await?;

    // 6. Deduplicate
    let existing = writer.find_actor_by_name(&name).await?;

    let actor_type = match extraction.actor_type.as_deref() {
        Some("individual") => ActorType::Individual,
        Some("government_body") => ActorType::GovernmentBody,
        Some("coalition") => ActorType::Coalition,
        _ => ActorType::Organization,
    };

    let entity_id = name.to_lowercase().replace(' ', "-");
    let actor = ActorNode {
        id: existing.unwrap_or_else(Uuid::new_v4),
        name: name.clone(),
        actor_type,
        entity_id,
        domains: vec![],
        social_urls: social_links.iter().map(|(_, _, u)| u.clone()).collect(),
        description: extraction.bio.clone().unwrap_or_default(),
        signal_count: 0,
        first_seen: chrono::Utc::now(),
        last_active: chrono::Utc::now(),
        typical_roles: vec![],
        bio: extraction.bio,
        location_lat: Some(lat),
        location_lng: Some(lng),
        location_name: Some(display_name.clone()),
    };

    let actor_id = writer.upsert_actor_with_profile(&actor).await?;

    // 7. Create sources for each social link
    for (_, _, social_url) in &social_links {
        let cv = rootsignal_common::canonical_value(social_url);
        let source = SourceNode {
            id: Uuid::new_v4(),
            canonical_key: cv.clone(),
            canonical_value: cv.clone(),
            url: Some(social_url.clone()),
            discovery_method: DiscoveryMethod::ActorAccount,
            created_at: chrono::Utc::now(),
            last_scraped: None,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context: Some(format!("Actor account: {name}")),
            weight: 0.7,
            cadence_hours: Some(12),
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: SourceRole::Mixed,
            scrape_count: 0,
        };
        if let Err(e) = writer.upsert_source(&source).await {
            warn!(error = %e, "Failed to create actor source");
            continue;
        }
        if let Err(e) = writer.link_actor_account(actor_id, &cv).await {
            warn!(error = %e, "Failed to link actor account");
        }
    }

    info!(name = name.as_str(), location = display_name.as_str(), social_count = social_links.len(), "Actor created from page");

    Ok(Some(ActorDiscoveryResult {
        actor_id,
        location_name: display_name,
    }))
}
