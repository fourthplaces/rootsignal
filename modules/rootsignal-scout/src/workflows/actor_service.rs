//! Restate durable service for actor-related operations.
//!
//! Wraps actor creation (from page, manual, account linking) so that GraphQL
//! mutations become thin validate-dispatch-return shells.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    ActorNode, ActorType, DiscoveryMethod, SourceNode, SourceRole,
};
use rootsignal_graph::GraphWriter;

use super::types::{
    AddAccountRequest, AddAccountResult, CreateFromPageRequest, CreateFromPageResult,
    CreateManualActorRequest, CreateManualActorResult,
};
use super::{create_archive, ScoutDeps};

#[restate_sdk::service]
#[name = "ActorService"]
pub trait ActorService {
    async fn create_from_page(
        req: CreateFromPageRequest,
    ) -> Result<CreateFromPageResult, HandlerError>;
    async fn create_manual(
        req: CreateManualActorRequest,
    ) -> Result<CreateManualActorResult, HandlerError>;
    async fn add_account(req: AddAccountRequest) -> Result<AddAccountResult, HandlerError>;
}

pub struct ActorServiceImpl {
    deps: Arc<ScoutDeps>,
}

impl ActorServiceImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl ActorService for ActorServiceImpl {
    async fn create_from_page(
        &self,
        _ctx: Context<'_>,
        req: CreateFromPageRequest,
    ) -> Result<CreateFromPageResult, HandlerError> {
        let deps = self.deps.clone();
        let url = req.url;
        let fallback_region = req.fallback_region;
        let require_social_links = req.require_social_links;
        let region_center_lat = req.region_center_lat;
        let region_center_lng = req.region_center_lng;

        super::spawn_workflow("ActorService/create_from_page", async move {
            let archive = create_archive(&deps);
            let writer = GraphWriter::new(deps.graph_client.clone());

            let result = crate::discovery::actor_discovery::create_actor_from_page(
                &archive,
                &writer,
                &deps.anthropic_api_key,
                &url,
                &fallback_region,
                require_social_links,
                region_center_lat,
                region_center_lng,
            )
            .await?;

            Ok(match result {
                Some(r) => CreateFromPageResult {
                    actor_id: Some(r.actor_id.to_string()),
                    location_name: Some(r.location_name),
                },
                None => CreateFromPageResult {
                    actor_id: None,
                    location_name: None,
                },
            })
        })
        .await
    }

    async fn create_manual(
        &self,
        _ctx: Context<'_>,
        req: CreateManualActorRequest,
    ) -> Result<CreateManualActorResult, HandlerError> {
        let deps = self.deps.clone();

        super::spawn_workflow("ActorService/create_manual", async move {
            let writer = GraphWriter::new(deps.graph_client.clone());

            // Geocode location
            let (lat, lng, display_name) =
                crate::discovery::actor_discovery::geocode_location(&req.location).await?;

            let actor_type = match req.actor_type.as_deref() {
                Some("individual") => ActorType::Individual,
                Some("government_body") => ActorType::GovernmentBody,
                Some("coalition") => ActorType::Coalition,
                _ => ActorType::Organization,
            };

            let entity_id = req.name.to_lowercase().replace(' ', "-");
            let actor = ActorNode {
                id: Uuid::new_v4(),
                name: req.name.clone(),
                actor_type,
                entity_id,
                domains: vec![],
                social_urls: req.social_accounts.clone(),
                description: req.bio.clone().unwrap_or_default(),
                signal_count: 0,
                first_seen: chrono::Utc::now(),
                last_active: chrono::Utc::now(),
                typical_roles: vec![],
                bio: req.bio,
                location_lat: Some(lat),
                location_lng: Some(lng),
                location_name: Some(display_name.clone()),
            };

            let actor_id = writer.upsert_actor_with_profile(&actor).await?;

            // Link social accounts as Source nodes with HAS_ACCOUNT edges
            for url in &req.social_accounts {
                let url = url.trim().to_string();
                if url.is_empty() {
                    continue;
                }
                let cv = rootsignal_common::canonical_value(&url);
                let source = SourceNode {
                    id: Uuid::new_v4(),
                    canonical_key: cv.clone(),
                    canonical_value: cv.clone(),
                    url: Some(url),
                    discovery_method: DiscoveryMethod::ActorAccount,
                    created_at: chrono::Utc::now(),
                    last_scraped: None,
                    last_produced_signal: None,
                    signals_produced: 0,
                    signals_corroborated: 0,
                    consecutive_empty_runs: 0,
                    active: true,
                    gap_context: Some(format!("Actor account: {}", req.name)),
                    weight: 0.7,
                    cadence_hours: Some(12),
                    avg_signals_per_scrape: 0.0,
                    quality_penalty: 1.0,
                    source_role: SourceRole::Mixed,
                    scrape_count: 0,
                    center_lat: None,
                    center_lng: None,
                };
                if let Err(e) = writer.upsert_source(&source).await {
                    warn!(error = %e, "Failed to create actor source");
                    continue;
                }
                if let Err(e) = writer.link_actor_account(actor_id, &cv).await {
                    warn!(error = %e, "Failed to link actor account");
                }
            }

            info!(name = req.name.as_str(), location = display_name.as_str(), "Actor created via ActorService");

            Ok(CreateManualActorResult {
                actor_id: actor_id.to_string(),
                location_name: display_name,
            })
        })
        .await
    }

    async fn add_account(
        &self,
        _ctx: Context<'_>,
        req: AddAccountRequest,
    ) -> Result<AddAccountResult, HandlerError> {
        let deps = self.deps.clone();

        super::spawn_workflow("ActorService/add_account", async move {
            let writer = GraphWriter::new(deps.graph_client.clone());
            let actor_uuid = Uuid::parse_str(&req.actor_id)?;
            let url = req.url.trim().to_string();

            let cv = rootsignal_common::canonical_value(&url);
            let source = SourceNode {
                id: Uuid::new_v4(),
                canonical_key: cv.clone(),
                canonical_value: cv.clone(),
                url: Some(url.clone()),
                discovery_method: DiscoveryMethod::ActorAccount,
                created_at: chrono::Utc::now(),
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: true,
                gap_context: Some("Actor account".to_string()),
                weight: 0.7,
                cadence_hours: Some(12),
                avg_signals_per_scrape: 0.0,
                quality_penalty: 1.0,
                source_role: SourceRole::Mixed,
                scrape_count: 0,
                center_lat: None,
                center_lng: None,
            };

            writer.upsert_source(&source).await?;
            writer.link_actor_account(actor_uuid, &cv).await?;

            info!(actor_id = req.actor_id.as_str(), url = url.as_str(), "Actor account added via ActorService");

            Ok(AddAccountResult { success: true })
        })
        .await
    }
}
