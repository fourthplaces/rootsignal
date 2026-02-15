use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

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
pub struct GqlSignal {
    pub id: Uuid,
    pub signal_type: String,
    pub content: String,
    pub about: Option<String>,
    pub entity_id: Option<Uuid>,
    pub source_url: Option<String>,
    pub source_citation_url: Option<String>,
    pub institutional_source: Option<String>,
    pub confidence: f32,
    pub in_language: String,
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
            institutional_source: s.institutional_source,
            confidence: s.confidence,
            in_language: s.in_language,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlSignalConnection {
    pub nodes: Vec<GqlSignal>,
    pub total_count: i64,
}
