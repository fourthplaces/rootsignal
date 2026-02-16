use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::graphql::loaders::*;
use super::super::locations::types::GqlLocation;
use super::super::schedules::types::GqlSchedule;
use rootsignal_domains::signals::Signal;

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum SignalType {
    Ask,
    Give,
    Event,
    Informative,
}

impl SignalType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Give => "give",
            Self::Event => "event",
            Self::Informative => "informative",
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum FlagType {
    WrongType,
    WrongEntity,
    Expired,
    Spam,
}

impl FlagType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WrongType => "wrong_type",
            Self::WrongEntity => "wrong_entity",
            Self::Expired => "expired",
            Self::Spam => "spam",
        }
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct GqlSignal {
    pub id: Uuid,
    pub signal_type: String,
    pub content: String,
    pub about: Option<String>,
    pub entity_id: Option<Uuid>,
    pub source_url: Option<String>,
    pub source_citation_url: Option<String>,
    pub confidence: f32,
    pub in_language: String,
    pub broadcasted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Signal> for GqlSignal {
    fn from(s: Signal) -> Self {
        Self {
            id: s.id,
            signal_type: s.signal_type,
            content: s.content,
            about: s.about,
            entity_id: s.entity_id,
            source_url: s.source_url,
            source_citation_url: s.source_citation_url,
            confidence: s.confidence,
            in_language: s.in_language,
            broadcasted_at: s.broadcasted_at,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

#[ComplexObject]
impl GqlSignal {
    async fn locations(&self, ctx: &Context<'_>) -> Result<Vec<GqlLocation>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<LocationsForLoader>>();
        let key = PolymorphicKey("signal".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn schedules(&self, ctx: &Context<'_>) -> Result<Vec<GqlSchedule>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<SchedulesForLoader>>();
        let key = PolymorphicKey("signal".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlSignalConnection {
    pub nodes: Vec<GqlSignal>,
    pub total_count: i64,
}
