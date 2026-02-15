use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_domains::findings::{Connection, Finding, FindingEvidence};

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum FindingStatus {
    Emerging,
    Active,
    Declining,
    Resolved,
}

impl FindingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Emerging => "emerging",
            Self::Active => "active",
            Self::Declining => "declining",
            Self::Resolved => "resolved",
        }
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct GqlFinding {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub validation_status: Option<String>,
    pub signal_velocity: Option<f32>,
    pub investigation_id: Option<Uuid>,
    pub trigger_signal_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlFinding {
    async fn evidence(&self, ctx: &Context<'_>) -> Result<Vec<GqlFindingEvidence>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let evidence = FindingEvidence::find_by_finding(self.id, pool).await?;
        Ok(evidence.into_iter().map(GqlFindingEvidence::from).collect())
    }

    async fn connections(&self, ctx: &Context<'_>, role: Option<String>) -> Result<Vec<GqlConnection>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let connections = if let Some(ref role) = role {
            Connection::find_to_by_role("finding", self.id, role, pool).await?
        } else {
            Connection::find_to("finding", self.id, pool).await?
        };
        Ok(connections.into_iter().map(GqlConnection::from).collect())
    }

    async fn connection_count(&self, ctx: &Context<'_>) -> Result<i64> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        Ok(Finding::connection_count(self.id, pool).await?)
    }
}

impl From<Finding> for GqlFinding {
    fn from(f: Finding) -> Self {
        Self {
            id: f.id,
            title: f.title,
            summary: f.summary,
            status: f.status,
            validation_status: f.validation_status,
            signal_velocity: f.signal_velocity,
            investigation_id: f.investigation_id,
            trigger_signal_id: f.trigger_signal_id,
            created_at: f.created_at,
            updated_at: f.updated_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlFindingEvidence {
    pub id: Uuid,
    pub finding_id: Uuid,
    pub evidence_type: String,
    pub quote: String,
    pub attribution: Option<String>,
    pub url: Option<String>,
    pub page_snapshot_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl From<FindingEvidence> for GqlFindingEvidence {
    fn from(e: FindingEvidence) -> Self {
        Self {
            id: e.id,
            finding_id: e.finding_id,
            evidence_type: e.evidence_type,
            quote: e.quote,
            attribution: e.attribution,
            url: e.url,
            page_snapshot_id: e.page_snapshot_id,
            created_at: e.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlConnection {
    pub id: Uuid,
    pub from_type: String,
    pub from_id: Uuid,
    pub to_type: String,
    pub to_id: Uuid,
    pub role: String,
    pub causal_quote: Option<String>,
    pub confidence: Option<f32>,
    pub created_at: DateTime<Utc>,
}

impl From<Connection> for GqlConnection {
    fn from(c: Connection) -> Self {
        Self {
            id: c.id,
            from_type: c.from_type,
            from_id: c.from_id,
            to_type: c.to_type,
            to_id: c.to_id,
            role: c.role,
            causal_quote: c.causal_quote,
            confidence: c.confidence,
            created_at: c.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlFindingConnection {
    pub nodes: Vec<GqlFinding>,
    pub total_count: i64,
}
