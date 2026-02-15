use restate_sdk::prelude::*;
use rootsignal_core::ServerDeps;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::taxonomy::{Tag, TagKindConfig};

// ─── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest {}
impl_restate_serde!(EmptyRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagKindJson {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    pub required: bool,
    pub is_public: bool,
    pub tag_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagKindListResult {
    pub kinds: Vec<TagKindJson>,
}
impl_restate_serde!(TagKindListResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTagsRequest {
    pub kind: String,
}
impl_restate_serde!(ListTagsRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagJson {
    pub id: String,
    pub kind: String,
    pub value: String,
    pub display_name: Option<String>,
}

impl From<Tag> for TagJson {
    fn from(t: Tag) -> Self {
        Self {
            id: t.id.to_string(),
            kind: t.kind,
            value: t.value,
            display_name: t.display_name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagListResult {
    pub tags: Vec<TagJson>,
}
impl_restate_serde!(TagListResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTagRequest {
    pub kind: String,
    pub value: String,
    pub display_name: Option<String>,
}
impl_restate_serde!(CreateTagRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagResult {
    pub tag: TagJson,
}
impl_restate_serde!(TagResult);

// ─── TagsService ────────────────────────────────────────────────────────────

#[restate_sdk::service]
#[name = "Tags"]
pub trait TagsService {
    async fn list_kinds(req: EmptyRequest) -> Result<TagKindListResult, HandlerError>;
    async fn list_tags(req: ListTagsRequest) -> Result<TagListResult, HandlerError>;
    async fn create_tag(req: CreateTagRequest) -> Result<TagResult, HandlerError>;
}

pub struct TagsServiceImpl {
    deps: Arc<ServerDeps>,
}

impl TagsServiceImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl TagsService for TagsServiceImpl {
    async fn list_kinds(
        &self,
        _ctx: Context<'_>,
        _req: EmptyRequest,
    ) -> Result<TagKindListResult, HandlerError> {
        let pool = self.deps.pool();

        let kinds = TagKindConfig::find_all(pool)
            .await
            .map_err(|e| TerminalError::new(format!("Query failed: {}", e)))?;

        let mut results = Vec::new();
        for kind in kinds {
            let count = TagKindConfig::tag_count_for_slug(&kind.slug, pool)
                .await
                .unwrap_or(0);

            results.push(TagKindJson {
                id: kind.id.to_string(),
                slug: kind.slug,
                display_name: kind.display_name,
                description: kind.description,
                required: kind.required,
                is_public: kind.is_public,
                tag_count: count,
            });
        }

        Ok(TagKindListResult { kinds: results })
    }

    async fn list_tags(
        &self,
        _ctx: Context<'_>,
        req: ListTagsRequest,
    ) -> Result<TagListResult, HandlerError> {
        let pool = self.deps.pool();

        let tags = Tag::find_by_kind(&req.kind, pool)
            .await
            .map_err(|e| TerminalError::new(format!("Query failed: {}", e)))?;

        Ok(TagListResult {
            tags: tags.into_iter().map(Into::into).collect(),
        })
    }

    async fn create_tag(
        &self,
        _ctx: Context<'_>,
        req: CreateTagRequest,
    ) -> Result<TagResult, HandlerError> {
        let pool = self.deps.pool();

        let tag = Tag::find_or_create(&req.kind, &req.value, pool)
            .await
            .map_err(|e| TerminalError::new(format!("Create failed: {}", e)))?;

        Ok(TagResult { tag: tag.into() })
    }
}
