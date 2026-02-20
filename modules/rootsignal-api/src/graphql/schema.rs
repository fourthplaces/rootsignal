use std::sync::Arc;

use async_graphql::dataloader::DataLoader;
use async_graphql::{Context, EmptySubscription, Object, Result, Schema, SimpleObject};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::{CityNode, Node, NodeType};
use rootsignal_graph::{CachedReader, GraphWriter};

use super::context::{AdminGuard, AuthContext};
use super::loaders::{
    ActorsBySignalLoader, EvidenceBySignalLoader, StoryBySignalLoader, TagsByStoryLoader,
};
use super::mutations::MutationRoot;
use super::types::*;

pub type ApiSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    // ========== Public queries ==========

    /// Check auth status. Returns claims info if authenticated.
    async fn me(&self, ctx: &Context<'_>) -> Option<MeResult> {
        let auth = ctx.data_unchecked::<AuthContext>();
        auth.0.as_ref().map(|c| MeResult {
            is_admin: c.is_admin,
            phone_number: c.phone_number.clone(),
        })
    }

    /// Find signals near a geographic point.
    async fn signals_near(
        &self,
        ctx: &Context<'_>,
        lat: f64,
        lng: f64,
        radius_km: f64,
        types: Option<Vec<SignalType>>,
    ) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let node_types: Option<Vec<NodeType>> =
            types.map(|t| t.into_iter().map(|st| st.to_node_type()).collect());
        let radius = radius_km.min(50.0);
        let nodes = reader
            .find_nodes_near(lat, lng, radius, node_types.as_deref())
            .await?;
        Ok(nodes.into_iter().map(GqlSignal::from).collect())
    }

    /// Find signals near a point, returned as a GeoJSON FeatureCollection string.
    async fn signals_near_geo_json(
        &self,
        ctx: &Context<'_>,
        lat: f64,
        lng: f64,
        radius_km: f64,
        types: Option<Vec<SignalType>>,
    ) -> Result<String> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let node_types: Option<Vec<NodeType>> =
            types.map(|t| t.into_iter().map(|st| st.to_node_type()).collect());
        let radius = radius_km.min(50.0);
        let nodes = reader
            .find_nodes_near(lat, lng, radius, node_types.as_deref())
            .await?;
        Ok(serde_json::to_string(&nodes_to_geojson(&nodes))?)
    }

    /// Get story signals as a GeoJSON FeatureCollection string.
    async fn story_signals_geo_json(&self, ctx: &Context<'_>, story_id: Uuid) -> Result<String> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let signals = reader.get_story_signals(story_id).await?;
        Ok(serde_json::to_string(&nodes_to_geojson(&signals))?)
    }

    /// List recent signals, ordered by triangulation quality.
    async fn signals_recent(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
        types: Option<Vec<SignalType>>,
    ) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let node_types: Option<Vec<NodeType>> =
            types.map(|t| t.into_iter().map(|st| st.to_node_type()).collect());
        let limit = limit.unwrap_or(50).min(200);
        let nodes = reader.list_recent(limit, node_types.as_deref()).await?;
        Ok(nodes.into_iter().map(GqlSignal::from).collect())
    }

    /// Get a single signal by ID.
    async fn signal(&self, ctx: &Context<'_>, id: Uuid) -> Result<Option<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let node = reader.get_signal_by_id(id).await?;
        Ok(node.map(GqlSignal::from))
    }

    // ========== Search app queries (public, no auth) ==========

    /// Find signals within a bounding box, sorted by heat. For viewport-driven browsing.
    async fn signals_in_bounds(
        &self,
        ctx: &Context<'_>,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: Option<u32>,
    ) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(50).min(200);
        let nodes = reader
            .signals_in_bounds(min_lat, max_lat, min_lng, max_lng, limit)
            .await?;
        Ok(nodes.into_iter().map(GqlSignal::from).collect())
    }

    /// Find stories within a bounding box (by centroid), sorted by energy.
    /// Optionally filter by tag slug.
    async fn stories_in_bounds(
        &self,
        ctx: &Context<'_>,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        tag: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(20).min(100);
        let stories = reader
            .stories_in_bounds_filtered(
                min_lat,
                max_lat,
                min_lng,
                max_lng,
                tag.as_deref(),
                limit,
            )
            .await?;
        Ok(stories.into_iter().map(GqlStory).collect())
    }

    /// Semantic search for signals within a bounding box. Embeds the query via Voyage AI,
    /// then finds nearest signals via vector KNN, post-filtered by bbox.
    async fn search_signals_in_bounds(
        &self,
        ctx: &Context<'_>,
        query: String,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: Option<u32>,
    ) -> Result<Vec<GqlSearchResult>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let embedder = ctx.data_unchecked::<Arc<rootsignal_scout::embedder::Embedder>>();
        let limit = limit.unwrap_or(50).min(200);

        let embedding = embedder.embed(&query).await.map_err(|e| {
            async_graphql::Error::new(format!("Embedding failed: {e}"))
        })?;

        let results = reader
            .semantic_search_signals_in_bounds(
                &embedding, min_lat, max_lat, min_lng, max_lng, limit,
            )
            .await?;

        Ok(results
            .into_iter()
            .map(|(node, score)| GqlSearchResult {
                signal: GqlSignal::from(node),
                score,
            })
            .collect())
    }

    /// Semantic search for stories within a bounding box. Searches signals via KNN,
    /// then aggregates to parent stories.
    async fn search_stories_in_bounds(
        &self,
        ctx: &Context<'_>,
        query: String,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: Option<u32>,
    ) -> Result<Vec<GqlStorySearchResult>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let embedder = ctx.data_unchecked::<Arc<rootsignal_scout::embedder::Embedder>>();
        let limit = limit.unwrap_or(20).min(100);

        let embedding = embedder.embed(&query).await.map_err(|e| {
            async_graphql::Error::new(format!("Embedding failed: {e}"))
        })?;

        let results = reader
            .semantic_search_stories_in_bounds(
                &embedding, min_lat, max_lat, min_lng, max_lng, limit,
            )
            .await?;

        Ok(results
            .into_iter()
            .map(|(story, score, top_title)| GqlStorySearchResult {
                story: GqlStory(story),
                score,
                top_matching_signal_title: if top_title.is_empty() {
                    None
                } else {
                    Some(top_title)
                },
            })
            .collect())
    }

    /// List stories ordered by energy.
    async fn stories(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
        status: Option<String>,
    ) -> Result<Vec<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(20).min(100);
        let stories = reader
            .top_stories_by_energy(limit, status.as_deref())
            .await?;
        Ok(stories.into_iter().map(GqlStory).collect())
    }

    /// Get a single story by ID.
    async fn story(&self, ctx: &Context<'_>, id: Uuid) -> Result<Option<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let story = reader.get_story_by_id(id).await?;
        Ok(story.map(GqlStory))
    }

    /// List stories by category.
    async fn stories_by_category(
        &self,
        ctx: &Context<'_>,
        category: String,
        limit: Option<u32>,
    ) -> Result<Vec<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(20).min(100);
        let stories = reader.stories_by_category(&category, limit).await?;
        Ok(stories.into_iter().map(GqlStory).collect())
    }

    /// List stories by arc.
    async fn stories_by_arc(
        &self,
        ctx: &Context<'_>,
        arc: String,
        limit: Option<u32>,
    ) -> Result<Vec<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(20).min(100);
        let stories = reader.stories_by_arc(&arc, limit).await?;
        Ok(stories.into_iter().map(GqlStory).collect())
    }

    /// List available tags, sorted by story count.
    async fn tags(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlTag>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(50).min(200) as usize;
        let tags = reader.top_tags(limit).await?;
        Ok(tags.into_iter().map(GqlTag).collect())
    }

    /// Stories that have a specific tag, optionally bounded geographically.
    async fn stories_by_tag(
        &self,
        ctx: &Context<'_>,
        tag: String,
        min_lat: Option<f64>,
        max_lat: Option<f64>,
        min_lng: Option<f64>,
        max_lng: Option<f64>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(20).min(100);
        let stories = reader
            .stories_by_tag(&tag, min_lat, max_lat, min_lng, max_lng, limit)
            .await?;
        Ok(stories.into_iter().map(GqlStory).collect())

    }

    /// Find tensions with < 2 respondents, not yet in any story, within bounds.
    async fn unresponded_tensions_in_bounds(
        &self,
        ctx: &Context<'_>,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: Option<u32>,
    ) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(20).min(100);
        let nodes = reader
            .unresponded_tensions_in_bounds(min_lat, max_lat, min_lng, max_lng, limit)
            .await?;
        Ok(nodes.into_iter().map(GqlSignal::from).collect())
    }

    /// List actors in a city.
    async fn actors(
        &self,
        ctx: &Context<'_>,
        city: String,
        limit: Option<u32>,
    ) -> Result<Vec<GqlActor>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(50).min(200);
        let actors = reader.actors_active_in_area(&city, limit).await?;
        Ok(actors.into_iter().map(GqlActor).collect())
    }

    /// Get a single actor by ID.
    async fn actor(&self, ctx: &Context<'_>, id: Uuid) -> Result<Option<GqlActor>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let actor = reader.actor_detail(id).await?;
        Ok(actor.map(GqlActor))
    }

    // ========== Admin queries (AdminGuard) ==========

    /// Dashboard data for a region.
    #[graphql(guard = "AdminGuard")]
    async fn admin_dashboard(&self, ctx: &Context<'_>, region: String) -> Result<AdminDashboardData> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();

        let (
            total_signals,
            story_count,
            actor_count,
            by_type,
            freshness,
            confidence,
            signal_volume,
            story_arcs,
            story_categories,
            tensions,
            discovery,
            yield_data,
            gap_stats,
            sources,
            all_cities,
        ) = tokio::join!(
            reader.total_count(),
            reader.story_count(),
            reader.actor_count(),
            reader.count_by_type(),
            reader.freshness_distribution(),
            reader.confidence_distribution(),
            reader.signal_volume_by_day(),
            reader.story_count_by_arc(),
            reader.story_count_by_category(),
            writer.get_unmet_tensions(20),
            writer.get_discovery_performance(&region),
            writer.get_extraction_yield(&region),
            writer.get_gap_type_stats(&region),
            writer.get_active_sources(&region),
            writer.list_regions(),
        );

        let sources = sources.unwrap_or_default();
        let (top_sources, bottom_sources) = discovery.unwrap_or_default();
        let all_regions = all_cities.unwrap_or_default();

        // Build scout status for each region using batch queries (2 queries instead of 2N)
        let region_slugs: Vec<String> = all_regions.iter().map(|c| c.slug.clone()).collect();
        let (running_map, due_map) = tokio::join!(
            writer.batch_scout_running(&region_slugs),
            writer.batch_due_sources(&region_slugs),
        );
        let running_map = running_map.unwrap_or_default();
        let due_map = due_map.unwrap_or_default();

        let mut scout_statuses = Vec::new();
        for c in &all_regions {
            scout_statuses.push(RegionScoutStatus {
                region_name: c.name.clone(),
                region_slug: c.slug.clone(),
                last_scouted: c.last_scout_completed_at,
                sources_due: *due_map.get(&c.slug).unwrap_or(&0),
                running: *running_map.get(&c.slug).unwrap_or(&false),
            });
        }

        let by_type = by_type.unwrap_or_default();
        let signal_volume = signal_volume.unwrap_or_default();

        Ok(AdminDashboardData {
            total_signals: total_signals.unwrap_or(0),
            total_stories: story_count.unwrap_or(0),
            total_actors: actor_count.unwrap_or(0),
            total_sources: sources.len() as u64,
            active_sources: sources.iter().filter(|s| s.active).count() as u64,
            total_tensions: tensions.as_ref().map(|t| t.len() as u64).unwrap_or(0),
            scout_statuses,
            signal_volume_by_day: signal_volume
                .iter()
                .map(|(day, ev, gi, need, not, ten)| DayVolume {
                    day: day.clone(),
                    gatherings: *ev,
                    aids: *gi,
                    needs: *need,
                    notices: *not,
                    tensions: *ten,
                })
                .collect(),
            count_by_type: by_type
                .iter()
                .map(|(t, c)| TypeCount {
                    signal_type: format!("{t}"),
                    count: *c,
                })
                .collect(),
            story_count_by_arc: story_arcs
                .unwrap_or_default()
                .iter()
                .map(|(arc, c)| LabelCount {
                    label: arc.clone(),
                    count: *c,
                })
                .collect(),
            story_count_by_category: story_categories
                .unwrap_or_default()
                .iter()
                .map(|(cat, c)| LabelCount {
                    label: cat.clone(),
                    count: *c,
                })
                .collect(),
            freshness_distribution: freshness
                .unwrap_or_default()
                .iter()
                .map(|(bucket, c)| LabelCount {
                    label: bucket.clone(),
                    count: *c,
                })
                .collect(),
            confidence_distribution: confidence
                .unwrap_or_default()
                .iter()
                .map(|(bucket, c)| LabelCount {
                    label: bucket.clone(),
                    count: *c,
                })
                .collect(),
            unmet_tensions: tensions
                .unwrap_or_default()
                .iter()
                .filter(|t| t.unmet)
                .map(|t| AdminTensionRow {
                    title: t.title.clone(),
                    severity: t.severity.clone(),
                    category: t.category.clone(),
                    what_would_help: t.what_would_help.clone(),
                })
                .collect(),
            top_sources: top_sources
                .iter()
                .take(10)
                .map(|s| AdminSourceRow {
                    name: s.canonical_value.clone(),
                    signals: s.signals_produced,
                    weight: s.weight,
                    empty_runs: s.consecutive_empty_runs,
                })
                .collect(),
            bottom_sources: bottom_sources
                .iter()
                .take(10)
                .map(|s| AdminSourceRow {
                    name: s.canonical_value.clone(),
                    signals: s.signals_produced,
                    weight: s.weight,
                    empty_runs: s.consecutive_empty_runs,
                })
                .collect(),
            extraction_yield: yield_data
                .unwrap_or_default()
                .iter()
                .map(|y| AdminYieldRow {
                    source_label: y.source_label.clone(),
                    extracted: y.extracted,
                    survived: y.survived,
                    corroborated: y.corroborated,
                    contradicted: y.contradicted,
                })
                .collect(),
            gap_stats: gap_stats
                .unwrap_or_default()
                .iter()
                .map(|g| AdminGapRow {
                    gap_type: g.gap_type.clone(),
                    total: g.total_sources,
                    successful: g.successful_sources,
                    avg_weight: g.avg_weight,
                })
                .collect(),
        })
    }

    /// List all regions with metadata.
    #[graphql(guard = "AdminGuard")]
    async fn admin_regions(&self, ctx: &Context<'_>) -> Result<Vec<AdminRegion>> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let regions = writer.list_regions().await?;

        let region_slugs: Vec<String> = regions.iter().map(|c| c.slug.clone()).collect();
        let (running_map, due_map) = tokio::join!(
            writer.batch_scout_running(&region_slugs),
            writer.batch_due_sources(&region_slugs),
        );
        let running_map = running_map.unwrap_or_default();
        let due_map = due_map.unwrap_or_default();

        let results = regions
            .iter()
            .map(|c| {
                let running = *running_map.get(&c.slug).unwrap_or(&false);
                let due = *due_map.get(&c.slug).unwrap_or(&0);
                AdminRegion::from_region_node(c, running, due)
            })
            .collect();
        Ok(results)
    }

    /// Get region detail.
    #[graphql(guard = "AdminGuard")]
    async fn admin_region(&self, ctx: &Context<'_>, slug: String) -> Result<Option<AdminRegion>> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let region = writer.get_region(&slug).await?;
        match region {
            Some(c) => {
                let running = writer.is_scout_running(&c.slug).await.unwrap_or(false);
                let due = writer.count_due_sources(&c.slug).await.unwrap_or(0);
                Ok(Some(AdminRegion::from_region_node(&c, running, due)))
            }
            None => Ok(None),
        }
    }

    /// List active sources for a region with schedule preview.
    #[graphql(guard = "AdminGuard")]
    async fn admin_region_sources(
        &self,
        ctx: &Context<'_>,
        region_slug: String,
    ) -> Result<Vec<AdminSource>> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let sources = writer.get_active_sources(&region_slug).await?;
        Ok(sources
            .iter()
            .map(|s| {
                let effective_weight = s.weight * s.quality_penalty;
                let cadence = s.cadence_hours.unwrap_or_else(|| {
                    rootsignal_scout::scheduler::cadence_hours_for_weight(effective_weight)
                });
                let source_label = source_label_from_value(s.value());
                AdminSource {
                    id: s.id,
                    url: s.url.clone().unwrap_or_default(),
                    canonical_value: s.canonical_value.clone(),
                    source_label,
                    weight: s.weight,
                    quality_penalty: s.quality_penalty,
                    effective_weight,
                    discovery_method: format!("{:?}", s.discovery_method),
                    last_scraped: s.last_scraped,
                    cadence_hours: cadence as f64,
                    signals_produced: s.signals_produced,
                    active: s.active,
                }
            })
            .collect())
    }

    /// Scout status for a specific region.
    #[graphql(guard = "AdminGuard")]
    async fn admin_scout_status(
        &self,
        ctx: &Context<'_>,
        region_slug: String,
    ) -> Result<RegionScoutStatus> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let region = writer.get_region(&region_slug).await?;
        let running = writer.is_scout_running(&region_slug).await.unwrap_or(false);
        let due = writer.count_due_sources(&region_slug).await.unwrap_or(0);

        Ok(RegionScoutStatus {
            region_name: region.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
            region_slug,
            last_scouted: region.and_then(|c| c.last_scout_completed_at),
            sources_due: due,
            running,
        })
    }
}

// ========== Admin GQL Types ==========

#[derive(SimpleObject)]
pub struct MeResult {
    pub is_admin: bool,
    pub phone_number: String,
}

#[derive(SimpleObject)]
pub struct AdminDashboardData {
    pub total_signals: u64,
    pub total_stories: u64,
    pub total_actors: u64,
    pub total_sources: u64,
    pub active_sources: u64,
    pub total_tensions: u64,
    pub scout_statuses: Vec<RegionScoutStatus>,
    pub signal_volume_by_day: Vec<DayVolume>,
    pub count_by_type: Vec<TypeCount>,
    pub story_count_by_arc: Vec<LabelCount>,
    pub story_count_by_category: Vec<LabelCount>,
    pub freshness_distribution: Vec<LabelCount>,
    pub confidence_distribution: Vec<LabelCount>,
    pub unmet_tensions: Vec<AdminTensionRow>,
    pub top_sources: Vec<AdminSourceRow>,
    pub bottom_sources: Vec<AdminSourceRow>,
    pub extraction_yield: Vec<AdminYieldRow>,
    pub gap_stats: Vec<AdminGapRow>,
}

#[derive(SimpleObject)]
pub struct RegionScoutStatus {
    pub region_name: String,
    pub region_slug: String,
    pub last_scouted: Option<DateTime<Utc>>,
    pub sources_due: u32,
    pub running: bool,
}

#[derive(SimpleObject)]
pub struct DayVolume {
    pub day: String,
    pub gatherings: u64,
    pub aids: u64,
    pub needs: u64,
    pub notices: u64,
    pub tensions: u64,
}

#[derive(SimpleObject)]
pub struct TypeCount {
    pub signal_type: String,
    pub count: u64,
}

#[derive(SimpleObject)]
pub struct LabelCount {
    pub label: String,
    pub count: u64,
}

#[derive(SimpleObject)]
pub struct AdminTensionRow {
    pub title: String,
    pub severity: String,
    pub category: Option<String>,
    pub what_would_help: Option<String>,
}

#[derive(SimpleObject)]
pub struct AdminSourceRow {
    pub name: String,
    pub signals: u32,
    pub weight: f64,
    pub empty_runs: u32,
}

#[derive(SimpleObject)]
pub struct AdminYieldRow {
    pub source_label: String,
    pub extracted: u32,
    pub survived: u32,
    pub corroborated: u32,
    pub contradicted: u32,
}

#[derive(SimpleObject)]
pub struct AdminGapRow {
    pub gap_type: String,
    pub total: u32,
    pub successful: u32,
    pub avg_weight: f64,
}

#[derive(SimpleObject)]
pub struct AdminRegion {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub last_scout_completed_at: Option<DateTime<Utc>>,
    pub scout_running: bool,
    pub sources_due: u32,
}

impl AdminRegion {
    fn from_region_node(c: &CityNode, running: bool, due: u32) -> Self {
        Self {
            id: c.id,
            slug: c.slug.clone(),
            name: c.name.clone(),
            center_lat: c.center_lat,
            center_lng: c.center_lng,
            radius_km: c.radius_km,
            active: c.active,
            created_at: c.created_at,
            last_scout_completed_at: c.last_scout_completed_at,
            scout_running: running,
            sources_due: due,
        }
    }
}

#[derive(SimpleObject)]
pub struct AdminSource {
    pub id: Uuid,
    pub url: String,
    pub canonical_value: String,
    pub source_label: String,
    pub weight: f64,
    pub quality_penalty: f64,
    pub effective_weight: f64,
    pub discovery_method: String,
    pub last_scraped: Option<DateTime<Utc>>,
    pub cadence_hours: f64,
    pub signals_produced: u32,
    pub active: bool,
}

// ========== Helpers ==========

fn source_label_from_value(value: &str) -> String {
    if rootsignal_common::is_web_query(value) {
        return "search".to_string();
    }
    // Extract domain from URL or canonical value (e.g. "instagram.com/handle" → "instagram.com")
    let without_scheme = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .unwrap_or(value);
    let domain = without_scheme.split('/').next().unwrap_or(value);
    domain.strip_prefix("www.").unwrap_or(domain).to_string()
}

fn nodes_to_geojson(nodes: &[Node]) -> serde_json::Value {
    let features: Vec<serde_json::Value> = nodes
        .iter()
        .filter_map(|node| {
            let meta = node.meta()?;
            let loc = meta.location?;
            Some(serde_json::json!({
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [loc.lng, loc.lat]
                },
                "properties": {
                    "id": meta.id.to_string(),
                    "title": meta.title,
                    "summary": meta.summary,
                    "node_type": format!("{}", node.node_type()),
                    "confidence": meta.confidence,
                    "corroboration_count": meta.corroboration_count,
                    "source_diversity": meta.source_diversity,
                    "cause_heat": meta.cause_heat,
                }
            }))
        })
        .collect();

    serde_json::json!({
        "type": "FeatureCollection",
        "features": features,
    })
}

pub fn build_schema(
    reader: Arc<CachedReader>,
    writer: Arc<GraphWriter>,
    jwt_service: JwtService,
    config: Arc<Config>,
    twilio: Option<Arc<twilio::TwilioService>>,
    rate_limiter: super::mutations::RateLimiter,
    scout_cancel: Arc<std::sync::atomic::AtomicBool>,
    graph_client: Arc<rootsignal_graph::GraphClient>,
    cache_store: Arc<rootsignal_graph::CacheStore>,
) -> ApiSchema {
    use super::mutations::ScoutCancel;

    let evidence_loader = DataLoader::new(
        EvidenceBySignalLoader {
            reader: reader.clone(),
        },
        tokio::spawn,
    );
    let actors_loader = DataLoader::new(
        ActorsBySignalLoader {
            reader: reader.clone(),
        },
        tokio::spawn,
    );
    let story_loader = DataLoader::new(
        StoryBySignalLoader {
            reader: reader.clone(),
        },
        tokio::spawn,
    );
    let tags_loader = DataLoader::new(
        TagsByStoryLoader {
            reader: reader.clone(),
        },
        tokio::spawn,
    );

    // Create Voyage AI embedder for semantic search (if API key is available)
    let embedder = {
        let voyage_key = &config.voyage_api_key;
        if voyage_key.is_empty() {
            tracing::warn!("VOYAGE_API_KEY not set — semantic search queries will fail");
        }
        Arc::new(rootsignal_scout::embedder::Embedder::new(voyage_key))
    };

    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(reader)
        .data(writer)
        .data(jwt_service)
        .data(config)
        .data(twilio)
        .data(rate_limiter)
        .data(ScoutCancel(scout_cancel))
        .data(graph_client)
        .data(cache_store)
        .data(evidence_loader)
        .data(actors_loader)
        .data(story_loader)
        .data(tags_loader)
        .data(embedder)
        .finish()
}

use crate::jwt::JwtService;
use rootsignal_common::Config;
