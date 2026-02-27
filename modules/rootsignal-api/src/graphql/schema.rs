use std::sync::Arc;

use async_graphql::dataloader::DataLoader;
use async_graphql::{Context, EmptySubscription, Object, Result, Schema, SimpleObject};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::NodeType;
use rootsignal_graph::{CachedReader, GraphWriter};

use super::context::{AdminGuard, AuthContext};
use super::loaders::{
    ActorsBySignalLoader, CitationBySignalLoader, ScheduleBySignalLoader, SituationsBySignalLoader,
    TagsBySituationLoader,
};
use super::mutations::MutationRoot;
use super::types::*;
use crate::restate_client::RestateClient;

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
        let embedder = ctx.data_unchecked::<Arc<rootsignal_scout::infra::embedder::Embedder>>();
        let limit = limit.unwrap_or(50).min(200);

        let embedding = embedder
            .embed(&query)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Embedding failed: {e}")))?;

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

    /// List available tags, sorted by usage count.
    async fn tags(&self, ctx: &Context<'_>, limit: Option<u32>) -> Result<Vec<GqlTag>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(50).min(200) as usize;
        let tags = reader.top_tags(limit).await?;
        Ok(tags.into_iter().map(GqlTag).collect())
    }

    // ========== Situation queries ==========

    /// Top situations by temperature.
    async fn situations(&self, ctx: &Context<'_>, limit: Option<u32>) -> Result<Vec<GqlSituation>> {
        let client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();
        let reader = rootsignal_graph::PublicGraphReader::new(client.as_ref().clone());
        let limit = limit.unwrap_or(20).min(100);
        let situations = reader.situations(limit).await?;
        Ok(situations.into_iter().map(GqlSituation).collect())
    }

    /// Get a single situation by ID.
    async fn situation(&self, ctx: &Context<'_>, id: Uuid) -> Result<Option<GqlSituation>> {
        let client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();
        let reader = rootsignal_graph::PublicGraphReader::new(client.as_ref().clone());
        let situation = reader.situation_by_id(&id).await?;
        Ok(situation.map(GqlSituation))
    }

    /// Situations within a geographic bounding box.
    async fn situations_in_bounds(
        &self,
        ctx: &Context<'_>,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        arc: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlSituation>> {
        let client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();
        let reader = rootsignal_graph::PublicGraphReader::new(client.as_ref().clone());
        let limit = limit.unwrap_or(20).min(100);
        let situations = reader
            .situations_in_bounds(min_lat, max_lat, min_lng, max_lng, limit, arc.as_deref())
            .await?;
        Ok(situations.into_iter().map(GqlSituation).collect())
    }

    /// Situations filtered by arc.
    async fn situations_by_arc(
        &self,
        ctx: &Context<'_>,
        arc: String,
        limit: Option<u32>,
    ) -> Result<Vec<GqlSituation>> {
        let client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();
        let reader = rootsignal_graph::PublicGraphReader::new(client.as_ref().clone());
        let limit = limit.unwrap_or(20).min(100);
        let situations = reader.situations_by_arc(&arc, limit).await?;
        Ok(situations.into_iter().map(GqlSituation).collect())
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

    /// Find actors within a bounding box, sorted by last_active.
    async fn actors_in_bounds(
        &self,
        ctx: &Context<'_>,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: Option<u32>,
    ) -> Result<Vec<GqlActor>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(50).min(200);
        let actors = reader
            .actors_in_bounds(min_lat, max_lat, min_lng, max_lng, limit)
            .await?;
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
    async fn admin_dashboard(
        &self,
        ctx: &Context<'_>,
        region: String,
    ) -> Result<AdminDashboardData> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();
        let pub_reader = rootsignal_graph::PublicGraphReader::new(client.as_ref().clone());

        let (
            total_signals,
            situation_count,
            actor_count,
            by_type,
            freshness,
            confidence,
            signal_volume,
            situation_arcs,
            situation_categories,
            tensions,
            discovery,
            yield_data,
            gap_stats,
            sources,
            due_sources,
            region_running,
        ) = tokio::join!(
            reader.total_count(),
            pub_reader.situation_count(),
            reader.actor_count(),
            reader.count_by_type(),
            reader.freshness_distribution(),
            reader.confidence_distribution(),
            reader.signal_volume_by_day(),
            pub_reader.situation_count_by_arc(),
            pub_reader.situation_count_by_category(),
            writer.get_unmet_tensions(20),
            writer.get_discovery_performance(),
            writer.get_extraction_yield(),
            writer.get_gap_type_stats(),
            writer.get_active_sources(),
            writer.count_due_sources(),
            writer.is_region_task_running(&region),
        );

        let sources = sources.unwrap_or_default();
        let (top_sources, bottom_sources) = discovery.unwrap_or_default();

        let scout_statuses = vec![RegionScoutStatus {
            region_name: region.clone(),
            region_slug: region.clone(),
            last_scouted: None,
            sources_due: due_sources.unwrap_or(0),
            running: region_running.unwrap_or(false),
        }];

        let by_type = by_type.unwrap_or_default();
        let signal_volume = signal_volume.unwrap_or_default();

        Ok(AdminDashboardData {
            total_signals: total_signals.unwrap_or(0),
            total_situations: situation_count.unwrap_or(0),
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
            situation_count_by_arc: situation_arcs
                .unwrap_or_default()
                .iter()
                .map(|(arc, c)| LabelCount {
                    label: arc.clone(),
                    count: *c,
                })
                .collect(),
            situation_count_by_category: situation_categories
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

    /// List active sources with schedule preview.
    #[graphql(guard = "AdminGuard")]
    async fn admin_region_sources(&self, ctx: &Context<'_>) -> Result<Vec<AdminSource>> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let sources = writer.get_active_sources().await?;
        Ok(sources
            .iter()
            .map(|s| {
                let effective_weight = s.weight * s.quality_penalty;
                let cadence = s.cadence_hours.unwrap_or_else(|| {
                    rootsignal_scout::scheduling::scheduler::cadence_hours_for_weight(
                        effective_weight,
                    )
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
        let (running, due) = tokio::join!(
            writer.is_region_task_running(&region_slug),
            writer.count_due_sources(),
        );

        Ok(RegionScoutStatus {
            region_name: region_slug.clone(),
            region_slug,
            last_scouted: None,
            sources_due: due.unwrap_or(0),
            running: running.unwrap_or(false),
        })
    }

    /// List supervisor validation findings for a region.
    #[graphql(guard = "AdminGuard")]
    async fn supervisor_findings(
        &self,
        ctx: &Context<'_>,
        region: String,
        status: Option<String>,
        limit: Option<i32>,
    ) -> Result<Vec<SupervisorFinding>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let limit = limit.unwrap_or(100).min(500) as i64;
        let rows = reader
            .list_validation_issues(&region, status.as_deref(), limit)
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| SupervisorFinding {
                id: r.id,
                issue_type: r.issue_type,
                severity: r.severity,
                target_id: r.target_id,
                target_label: r.target_label,
                description: r.description,
                suggested_action: r.suggested_action,
                status: r.status,
                created_at: r.created_at,
                resolved_at: r.resolved_at,
            })
            .collect())
    }

    /// List recent scout runs for a region.
    #[graphql(guard = "AdminGuard")]
    async fn admin_scout_runs(
        &self,
        ctx: &Context<'_>,
        region: String,
        limit: Option<u32>,
    ) -> Result<Vec<ScoutRun>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;
        let limit = limit.unwrap_or(20).min(100);

        let rows = crate::db::scout_run::list_by_region(pool, &region, limit)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to query scout runs: {e}")))?;

        Ok(rows.into_iter().map(ScoutRun::from).collect())
    }

    /// Get a single scout run by run_id.
    #[graphql(guard = "AdminGuard")]
    async fn admin_scout_run(&self, ctx: &Context<'_>, run_id: String) -> Result<Option<ScoutRun>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let row = crate::db::scout_run::find_by_id(pool, &run_id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to query scout run: {e}")))?;

        Ok(row.map(ScoutRun::from))
    }

    /// Aggregate summary of supervisor findings for a region.
    #[graphql(guard = "AdminGuard")]
    async fn supervisor_summary(
        &self,
        ctx: &Context<'_>,
        region: String,
    ) -> Result<SupervisorSummary> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let summary = reader.validation_issue_summary(&region).await?;

        Ok(SupervisorSummary {
            total_open: summary.total_open,
            total_resolved: summary.total_resolved,
            total_dismissed: summary.total_dismissed,
            count_by_type: summary
                .count_by_type
                .into_iter()
                .map(|(label, count)| FindingCount { label, count })
                .collect(),
            count_by_severity: summary
                .count_by_severity
                .into_iter()
                .map(|(label, count)| FindingCount { label, count })
                .collect(),
        })
    }

    /// List scout tasks, optionally filtered by status.
    #[graphql(guard = "AdminGuard")]
    async fn admin_scout_tasks(
        &self,
        ctx: &Context<'_>,
        status: Option<String>,
        limit: Option<i32>,
    ) -> Result<Vec<GqlScoutTask>> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let lim = limit.unwrap_or(50).min(200) as u32;
        let tasks = writer
            .list_scout_tasks(status.as_deref(), lim)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to list scout tasks: {e}")))?;

        Ok(tasks.into_iter().map(GqlScoutTask::from_task).collect())
    }

    // ========== Archive queries ==========

    /// Total row counts for all archive content types.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_counts(&self, ctx: &Context<'_>) -> Result<GqlArchiveCounts> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let counts = crate::db::archive::count_all(pool).await.map_err(|e| {
            async_graphql::Error::new(format!("Failed to query archive counts: {e}"))
        })?;

        Ok(GqlArchiveCounts {
            posts: counts.posts,
            short_videos: counts.short_videos,
            stories: counts.stories,
            long_videos: counts.long_videos,
            pages: counts.pages,
            feeds: counts.feeds,
            search_results: counts.search_results,
            files: counts.files,
        })
    }

    /// Daily ingestion volume for the last N days, broken down by content type.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_volume(
        &self,
        ctx: &Context<'_>,
        days: Option<u32>,
    ) -> Result<Vec<GqlArchiveVolumeDay>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let days = days.unwrap_or(7);
        let rows = crate::db::archive::volume_by_day(pool, days)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to query archive volume: {e}"))
            })?;

        Ok(rows
            .into_iter()
            .map(|r| GqlArchiveVolumeDay {
                day: r.day,
                posts: r.posts,
                short_videos: r.short_videos,
                stories: r.stories,
                long_videos: r.long_videos,
                pages: r.pages,
                feeds: r.feeds,
                search_results: r.search_results,
                files: r.files,
            })
            .collect())
    }

    /// Recent posts from the archive.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_posts(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlArchivePost>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let limit = limit.unwrap_or(50);
        let rows = crate::db::archive::recent_posts(pool, limit)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to query archive posts: {e}"))
            })?;

        Ok(rows
            .into_iter()
            .map(|r| GqlArchivePost {
                id: r.id,
                source_url: r.source_url.clone(),
                permalink: r.permalink,
                author: r.author,
                text_preview: crate::db::archive::truncate_text(&r.text, 150),
                platform: crate::db::archive::platform_from_url(&r.source_url),
                hashtags: r.hashtags,
                engagement_summary: crate::db::archive::format_engagement(&r.engagement),
                published_at: r.published_at,
            })
            .collect())
    }

    /// Recent short videos (reels) from the archive.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_short_videos(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlArchiveShortVideo>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let limit = limit.unwrap_or(50);
        let rows = crate::db::archive::recent_short_videos(pool, limit)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to query archive short videos: {e}"))
            })?;

        Ok(rows
            .into_iter()
            .map(|r| GqlArchiveShortVideo {
                id: r.id,
                source_url: r.source_url.clone(),
                permalink: r.permalink,
                text_preview: crate::db::archive::truncate_text(&r.text, 150),
                engagement_summary: crate::db::archive::format_engagement(&r.engagement),
                published_at: r.published_at,
            })
            .collect())
    }

    /// Recent stories from the archive.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_stories(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlArchiveStory>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let limit = limit.unwrap_or(50);
        let rows = crate::db::archive::recent_stories(pool, limit)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to query archive stories: {e}"))
            })?;

        Ok(rows
            .into_iter()
            .map(|r| GqlArchiveStory {
                id: r.id,
                source_url: r.source_url,
                permalink: r.permalink,
                text_preview: crate::db::archive::truncate_text(&r.text, 150),
                location: r.location,
                expires_at: r.expires_at,
                fetched_at: r.fetched_at,
            })
            .collect())
    }

    /// Recent long videos from the archive.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_long_videos(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlArchiveLongVideo>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let limit = limit.unwrap_or(50);
        let rows = crate::db::archive::recent_long_videos(pool, limit)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to query archive long videos: {e}"))
            })?;

        Ok(rows
            .into_iter()
            .map(|r| GqlArchiveLongVideo {
                id: r.id,
                source_url: r.source_url.clone(),
                permalink: r.permalink,
                text_preview: crate::db::archive::truncate_text(&r.text, 150),
                engagement_summary: crate::db::archive::format_engagement(&r.engagement),
                published_at: r.published_at,
            })
            .collect())
    }

    /// Recent pages from the archive.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_pages(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlArchivePage>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let limit = limit.unwrap_or(50);
        let rows = crate::db::archive::recent_pages(pool, limit)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to query archive pages: {e}"))
            })?;

        Ok(rows
            .into_iter()
            .map(|r| GqlArchivePage {
                id: r.id,
                source_url: r.source_url,
                title: r.title,
                fetched_at: r.fetched_at,
            })
            .collect())
    }

    /// Recent feeds from the archive.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_feeds(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlArchiveFeed>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let limit = limit.unwrap_or(50);
        let rows = crate::db::archive::recent_feeds(pool, limit)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to query archive feeds: {e}"))
            })?;

        Ok(rows
            .into_iter()
            .map(|r| GqlArchiveFeed {
                id: r.id,
                source_url: r.source_url,
                title: r.title,
                item_count: r.item_count,
                fetched_at: r.fetched_at,
            })
            .collect())
    }

    /// Recent search results from the archive.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_search_results(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlArchiveSearchResult>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let limit = limit.unwrap_or(50);
        let rows = crate::db::archive::recent_search_results(pool, limit)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to query archive search results: {e}"))
            })?;

        Ok(rows
            .into_iter()
            .map(|r| GqlArchiveSearchResult {
                id: r.id,
                query: r.query,
                result_count: r.result_count,
                fetched_at: r.fetched_at,
            })
            .collect())
    }

    /// Recent files from the archive.
    #[graphql(guard = "AdminGuard")]
    async fn admin_archive_files(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
    ) -> Result<Vec<GqlArchiveFile>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        let limit = limit.unwrap_or(50);
        let rows = crate::db::archive::recent_files(pool, limit)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to query archive files: {e}"))
            })?;

        Ok(rows
            .into_iter()
            .map(|r| GqlArchiveFile {
                id: r.id,
                url: r.url,
                title: r.title,
                mime_type: r.mime_type,
                duration: r.duration,
                page_count: r.page_count,
                fetched_at: r.fetched_at,
            })
            .collect())
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
    pub total_situations: u64,
    pub total_actors: u64,
    pub total_sources: u64,
    pub active_sources: u64,
    pub total_tensions: u64,
    pub scout_statuses: Vec<RegionScoutStatus>,
    pub signal_volume_by_day: Vec<DayVolume>,
    pub count_by_type: Vec<TypeCount>,
    pub situation_count_by_arc: Vec<LabelCount>,
    pub situation_count_by_category: Vec<LabelCount>,
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

// ========== Archive GQL Types ==========

#[derive(SimpleObject)]
struct GqlArchiveCounts {
    posts: i64,
    short_videos: i64,
    stories: i64,
    long_videos: i64,
    pages: i64,
    feeds: i64,
    search_results: i64,
    files: i64,
}

#[derive(SimpleObject)]
struct GqlArchiveVolumeDay {
    day: String,
    posts: i64,
    short_videos: i64,
    stories: i64,
    long_videos: i64,
    pages: i64,
    feeds: i64,
    search_results: i64,
    files: i64,
}

#[derive(SimpleObject)]
struct GqlArchivePost {
    id: Uuid,
    source_url: String,
    permalink: Option<String>,
    author: Option<String>,
    text_preview: Option<String>,
    platform: String,
    hashtags: Vec<String>,
    engagement_summary: String,
    published_at: Option<DateTime<Utc>>,
}

#[derive(SimpleObject)]
struct GqlArchiveShortVideo {
    id: Uuid,
    source_url: String,
    permalink: Option<String>,
    text_preview: Option<String>,
    engagement_summary: String,
    published_at: Option<DateTime<Utc>>,
}

#[derive(SimpleObject)]
struct GqlArchiveStory {
    id: Uuid,
    source_url: String,
    permalink: Option<String>,
    text_preview: Option<String>,
    location: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    fetched_at: DateTime<Utc>,
}

#[derive(SimpleObject)]
struct GqlArchiveLongVideo {
    id: Uuid,
    source_url: String,
    permalink: Option<String>,
    text_preview: Option<String>,
    engagement_summary: String,
    published_at: Option<DateTime<Utc>>,
}

#[derive(SimpleObject)]
struct GqlArchivePage {
    id: Uuid,
    source_url: String,
    title: Option<String>,
    fetched_at: DateTime<Utc>,
}

#[derive(SimpleObject)]
struct GqlArchiveFeed {
    id: Uuid,
    source_url: String,
    title: Option<String>,
    item_count: i64,
    fetched_at: DateTime<Utc>,
}

#[derive(SimpleObject)]
struct GqlArchiveSearchResult {
    id: Uuid,
    query: String,
    result_count: i64,
    fetched_at: DateTime<Utc>,
}

#[derive(SimpleObject)]
struct GqlArchiveFile {
    id: Uuid,
    url: String,
    title: Option<String>,
    mime_type: String,
    duration: Option<f64>,
    page_count: Option<i32>,
    fetched_at: DateTime<Utc>,
}

// ========== Scout Run Types ==========

use crate::db::scout_run::{EventRow, ScoutRunRow, StatsJson};

/// GraphQL output type for a scout run.
/// Events are loaded lazily — only queried when the client requests the `events` field.
struct ScoutRun {
    row: ScoutRunRow,
}

#[Object]
impl ScoutRun {
    async fn run_id(&self) -> &str {
        &self.row.run_id
    }
    async fn region(&self) -> &str {
        &self.row.region
    }
    async fn started_at(&self) -> DateTime<Utc> {
        self.row.started_at
    }
    async fn finished_at(&self) -> DateTime<Utc> {
        self.row.finished_at
    }
    async fn stats(&self) -> ScoutRunStats {
        ScoutRunStats::from(&self.row.stats)
    }

    async fn events(&self, ctx: &Context<'_>) -> Result<Vec<ScoutRunEvent>> {
        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;
        let rows = crate::db::scout_run::list_events_by_run_id(pool, &self.row.run_id, None)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to load events: {e}")))?;
        Ok(rows.into_iter().map(ScoutRunEvent::from).collect())
    }
}

#[derive(SimpleObject)]
struct ScoutRunStats {
    urls_scraped: u32,
    urls_unchanged: u32,
    urls_failed: u32,
    signals_extracted: u32,
    signals_deduplicated: u32,
    signals_stored: u32,
    social_media_posts: u32,
    expansion_queries_collected: u32,
    expansion_sources_created: u32,
}

#[derive(SimpleObject)]
struct ScoutRunEvent {
    seq: u32,
    ts: DateTime<Utc>,
    #[graphql(name = "type")]
    event_type: String,
    query: Option<String>,
    url: Option<String>,
    provider: Option<String>,
    platform: Option<String>,
    identifier: Option<String>,
    signal_type: Option<String>,
    title: Option<String>,
    result_count: Option<u32>,
    post_count: Option<u32>,
    items: Option<u32>,
    content_bytes: Option<u64>,
    content_chars: Option<u64>,
    signals_extracted: Option<u32>,
    implied_queries: Option<u32>,
    similarity: Option<f64>,
    confidence: Option<f64>,
    success: Option<bool>,
    action: Option<String>,
    node_id: Option<String>,
    matched_id: Option<String>,
    existing_id: Option<String>,
    source_url: Option<String>,
    new_source_url: Option<String>,
    canonical_key: Option<String>,
    gatherings: Option<u64>,
    needs: Option<u64>,
    stale: Option<u64>,
    sources_created: Option<u64>,
    spent_cents: Option<u64>,
    remaining_cents: Option<u64>,
    topics: Option<Vec<String>>,
    posts_found: Option<u32>,
    reason: Option<String>,
    strategy: Option<String>,
    field: Option<String>,
    old_value: Option<String>,
    new_value: Option<String>,
    signal_count: Option<u32>,
    summary: Option<String>,
}

impl From<ScoutRunRow> for ScoutRun {
    fn from(r: ScoutRunRow) -> Self {
        Self { row: r }
    }
}

impl From<&StatsJson> for ScoutRunStats {
    fn from(s: &StatsJson) -> Self {
        Self {
            urls_scraped: s.urls_scraped.unwrap_or(0),
            urls_unchanged: s.urls_unchanged.unwrap_or(0),
            urls_failed: s.urls_failed.unwrap_or(0),
            signals_extracted: s.signals_extracted.unwrap_or(0),
            signals_deduplicated: s.signals_deduplicated.unwrap_or(0),
            signals_stored: s.signals_stored.unwrap_or(0),
            social_media_posts: s.social_media_posts.unwrap_or(0),
            expansion_queries_collected: s.expansion_queries_collected.unwrap_or(0),
            expansion_sources_created: s.expansion_sources_created.unwrap_or(0),
        }
    }
}

impl From<EventRow> for ScoutRunEvent {
    fn from(j: EventRow) -> Self {
        Self {
            seq: j.seq as u32,
            ts: j.ts,
            event_type: j.event_type,
            query: j.query,
            url: j.url,
            provider: j.provider,
            platform: j.platform,
            identifier: j.identifier,
            signal_type: j.signal_type,
            title: j.title,
            result_count: j.result_count.map(|v| v as u32),
            post_count: j.post_count.map(|v| v as u32),
            items: j.items.map(|v| v as u32),
            content_bytes: j.content_bytes.map(|v| v as u64),
            content_chars: j.content_chars.map(|v| v as u64),
            signals_extracted: j.signals_extracted.map(|v| v as u32),
            implied_queries: j.implied_queries.map(|v| v as u32),
            similarity: j.similarity,
            confidence: j.confidence,
            success: j.success,
            action: j.action,
            node_id: j.node_id,
            matched_id: j.matched_id,
            existing_id: j.existing_id,
            source_url: j.source_url,
            new_source_url: j.new_source_url,
            canonical_key: j.canonical_key,
            gatherings: j.gatherings.map(|v| v as u64),
            needs: j.needs.map(|v| v as u64),
            stale: j.stale.map(|v| v as u64),
            sources_created: j.sources_created.map(|v| v as u64),
            spent_cents: j.spent_cents.map(|v| v as u64),
            remaining_cents: j.remaining_cents.map(|v| v as u64),
            topics: j.topics,
            posts_found: j.posts_found.map(|v| v as u32),
            reason: j.reason,
            strategy: j.strategy,
            field: j.field,
            old_value: j.old_value,
            new_value: j.new_value,
            signal_count: j.signal_count.map(|v| v as u32),
            summary: j.summary,
        }
    }
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

pub fn build_schema(
    reader: Arc<CachedReader>,
    writer: Arc<GraphWriter>,
    store_factory: Option<rootsignal_scout::store::SignalStoreFactory>,
    jwt_service: JwtService,
    config: Arc<Config>,
    twilio: Option<Arc<twilio::TwilioService>>,
    rate_limiter: super::mutations::RateLimiter,
    graph_client: Arc<rootsignal_graph::GraphClient>,
    cache_store: Arc<rootsignal_graph::CacheStore>,
    restate_client: Option<RestateClient>,
    pg_pool: Option<sqlx::PgPool>,
) -> ApiSchema {
    let citation_loader = DataLoader::new(
        CitationBySignalLoader {
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
    let situations_loader = DataLoader::new(
        SituationsBySignalLoader {
            reader: reader.clone(),
        },
        tokio::spawn,
    );
    let schedule_loader = DataLoader::new(
        ScheduleBySignalLoader {
            reader: reader.clone(),
        },
        tokio::spawn,
    );
    let situation_tags_loader = DataLoader::new(
        TagsBySituationLoader {
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
        Arc::new(rootsignal_scout::infra::embedder::Embedder::new(voyage_key))
    };

    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(reader)
        .data(store_factory)
        .data(writer)
        .data(jwt_service)
        .data(config)
        .data(twilio)
        .data(rate_limiter)
        .data(graph_client)
        .data(cache_store)
        .data(citation_loader)
        .data(actors_loader)
        .data(situations_loader)
        .data(schedule_loader)
        .data(situation_tags_loader)
        .data(embedder)
        .data(restate_client)
        .data(pg_pool)
        .finish()
}

use crate::jwt::JwtService;
use rootsignal_common::Config;
