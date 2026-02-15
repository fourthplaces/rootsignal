use restate_sdk::prelude::*;
use rootsignal_core::ServerDeps;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

// ─── Qualify types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualifyRequest {}
impl_restate_serde!(QualifyRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualifyResult {
    pub score: i32,
    pub verdict: String,
    pub reasoning: String,
}
impl_restate_serde!(QualifyResult);

// ─── QualifyWorkflow ────────────────────────────────────────────────────────

#[restate_sdk::workflow]
#[name = "QualifyWorkflow"]
pub trait QualifyWorkflow {
    async fn run(req: QualifyRequest) -> Result<QualifyResult, HandlerError>;

    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct QualifyWorkflowImpl {
    deps: Arc<ServerDeps>,
}

impl QualifyWorkflowImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl QualifyWorkflow for QualifyWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        _req: QualifyRequest,
    ) -> Result<QualifyResult, HandlerError> {
        // The workflow key IS the source_id
        let source_id: Uuid = ctx.key().parse().map_err(|e: uuid::Error| {
            TerminalError::new(format!("Invalid source UUID in workflow key: {}", e))
        })?;

        ctx.set("status", "qualifying".to_string());

        let deps = self.deps.clone();
        let result_json: String = ctx
            .run(|| async move {
                let result = crate::scraping::activities::qualify_source(source_id, &deps)
                    .await
                    .map_err(|e| TerminalError::new(format!("Qualification failed: {}", e)))?;
                serde_json::to_string(&result)
                    .map_err(|e| TerminalError::new(format!("Serialize failed: {}", e)).into())
            })
            .await?;

        let qualification: crate::scraping::activities::qualify_source::SourceQualification =
            serde_json::from_str(&result_json)
                .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

        ctx.set("status", "completed".to_string());

        Ok(QualifyResult {
            score: qualification.score,
            verdict: qualification.verdict,
            reasoning: qualification.reasoning,
        })
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        Ok(ctx
            .get::<String>("status")
            .await?
            .unwrap_or_else(|| "unknown".to_string()))
    }
}

// ─── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeRequest {
    pub source_id: String,
}
impl_restate_serde!(ScrapeRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResult {
    pub source_id: String,
    pub snapshot_ids: Vec<String>,
    pub status: String,
}
impl_restate_serde!(ScrapeResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartCycleRequest {}
impl_restate_serde!(StartCycleRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleResult {
    pub sources_scraped: u32,
    pub total_snapshots: u32,
}
impl_restate_serde!(CycleResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest {}
impl_restate_serde!(EmptyRequest);

// ─── ScrapeWorkflow ──────────────────────────────────────────────────────────

#[restate_sdk::workflow]
#[name = "ScrapeWorkflow"]
pub trait ScrapeWorkflow {
    async fn run(req: ScrapeRequest) -> Result<ScrapeResult, HandlerError>;

    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct ScrapeWorkflowImpl {
    deps: Arc<ServerDeps>,
}

impl ScrapeWorkflowImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl ScrapeWorkflow for ScrapeWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: ScrapeRequest,
    ) -> Result<ScrapeResult, HandlerError> {
        let source_id: Uuid = req
            .source_id
            .parse()
            .map_err(|e: uuid::Error| TerminalError::new(format!("Invalid UUID: {}", e)))?;

        ctx.set("status", "scraping".to_string());

        let deps = self.deps.clone();
        let snapshot_ids_json: String = ctx
            .run(|| async move {
                let ids = crate::scraping::activities::scrape_source(source_id, &deps)
                    .await
                    .map_err(|e| TerminalError::new(format!("Scrape failed: {}", e)))?;
                serde_json::to_string(&ids.iter().map(|id| id.to_string()).collect::<Vec<_>>())
                    .map_err(|e| TerminalError::new(format!("Serialize failed: {}", e)).into())
            })
            .await?;

        let snapshot_ids: Vec<String> = serde_json::from_str(&snapshot_ids_json)
            .map_err(|e| TerminalError::new(format!("Deserialize failed: {}", e)))?;

        ctx.set("status", "completed".to_string());

        Ok(ScrapeResult {
            source_id: req.source_id,
            snapshot_ids,
            status: "completed".to_string(),
        })
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        Ok(ctx
            .get::<String>("status")
            .await?
            .unwrap_or_else(|| "unknown".to_string()))
    }
}

// ─── SourceObject (virtual object — prevents concurrent scrapes) ─────────────

#[restate_sdk::object]
#[name = "Source"]
pub trait SourceObject {
    async fn scrape(req: ScrapeRequest) -> Result<ScrapeResult, HandlerError>;
}

pub struct SourceObjectImpl {
    deps: Arc<ServerDeps>,
}

impl SourceObjectImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl SourceObject for SourceObjectImpl {
    async fn scrape(
        &self,
        ctx: ObjectContext<'_>,
        req: ScrapeRequest,
    ) -> Result<ScrapeResult, HandlerError> {
        let source_id: Uuid = req
            .source_id
            .parse()
            .map_err(|e: uuid::Error| TerminalError::new(format!("Invalid UUID: {}", e)))?;

        let deps = self.deps.clone();
        let snapshot_ids_json: String = ctx
            .run(|| async move {
                let ids = crate::scraping::activities::scrape_source(source_id, &deps)
                    .await
                    .map_err(|e| TerminalError::new(format!("Scrape failed: {}", e)))?;
                serde_json::to_string(&ids.iter().map(|id| id.to_string()).collect::<Vec<_>>())
                    .map_err(|e| TerminalError::new(format!("Serialize failed: {}", e)).into())
            })
            .await?;

        let snapshot_ids: Vec<String> = serde_json::from_str(&snapshot_ids_json)
            .map_err(|e| TerminalError::new(format!("Deserialize failed: {}", e)))?;

        Ok(ScrapeResult {
            source_id: req.source_id,
            snapshot_ids,
            status: "completed".to_string(),
        })
    }
}

// ─── SchedulerService ────────────────────────────────────────────────────────

#[restate_sdk::service]
#[name = "SchedulerService"]
pub trait SchedulerService {
    async fn start_cycle(req: StartCycleRequest) -> Result<CycleResult, HandlerError>;
}

pub struct SchedulerServiceImpl {
    deps: Arc<ServerDeps>,
}

impl SchedulerServiceImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl SchedulerService for SchedulerServiceImpl {
    async fn start_cycle(
        &self,
        ctx: Context<'_>,
        _req: StartCycleRequest,
    ) -> Result<CycleResult, HandlerError> {
        let deps = self.deps.clone();

        let sources_json: String = ctx
            .run(|| async move {
                let sources = crate::scraping::Source::find_due_for_scrape(deps.pool())
                    .await
                    .map_err(|e| TerminalError::new(format!("Failed to find sources: {}", e)))?;
                let ids: Vec<String> = sources.iter().map(|s| s.id.to_string()).collect();
                serde_json::to_string(&ids)
                    .map_err(|e| TerminalError::new(format!("Serialize failed: {}", e)).into())
            })
            .await?;

        let sources: Vec<String> = serde_json::from_str(&sources_json)
            .map_err(|e| TerminalError::new(format!("Deserialize failed: {}", e)))?;

        tracing::info!(count = sources.len(), "Starting scrape cycle");

        let mut total_snapshots: u32 = 0;
        let sources_count = sources.len() as u32;

        for source_id_str in &sources {
            let result: ScrapeResult = ctx
                .object_client::<SourceObjectClient>(source_id_str)
                .scrape(ScrapeRequest {
                    source_id: source_id_str.clone(),
                })
                .call()
                .await?;
            total_snapshots += result.snapshot_ids.len() as u32;
        }

        Ok(CycleResult {
            sources_scraped: sources_count,
            total_snapshots,
        })
    }
}
