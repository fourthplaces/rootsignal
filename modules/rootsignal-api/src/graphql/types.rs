// async-graphql's #[Object] proc macro transforms methods into trait impls,
// causing false-positive dead_code warnings on helpers called from macro-expanded resolvers.
#![allow(dead_code)]

use std::sync::Arc;

use async_graphql::dataloader::DataLoader;
use async_graphql::{Context, Object, Result, SimpleObject, Union};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::{
    ActorNode, AidNode, EvidenceNode, GatheringNode, NeedNode, Node, NodeMeta, NoticeNode,
    StoryNode, TagNode, TensionNode,
};
use rootsignal_graph::CachedReader;

use super::loaders::{
    ActorsBySignalLoader, EvidenceBySignalLoader, SituationsBySignalLoader, StoryBySignalLoader,
    TagsBySituationLoader, TagsByStoryLoader,
};

// --- GraphQL Enums ---

#[derive(async_graphql::Enum, Debug, Copy, Clone, Eq, PartialEq)]
pub enum ScoutPhase {
    Bootstrap,
    Scrape,
    Synthesis,
    SituationWeaver,
    Supervisor,
}

impl From<ScoutPhase> for crate::restate_client::ScoutPhase {
    fn from(gql: ScoutPhase) -> Self {
        match gql {
            ScoutPhase::Bootstrap => Self::Bootstrap,
            ScoutPhase::Scrape => Self::Scrape,
            ScoutPhase::Synthesis => Self::Synthesis,
            ScoutPhase::SituationWeaver => Self::SituationWeaver,
            ScoutPhase::Supervisor => Self::Supervisor,
        }
    }
}

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum SignalType {
    Gathering,
    Aid,
    Need,
    Notice,
    Tension,
}

impl From<rootsignal_common::NodeType> for SignalType {
    fn from(nt: rootsignal_common::NodeType) -> Self {
        match nt {
            rootsignal_common::NodeType::Gathering => SignalType::Gathering,
            rootsignal_common::NodeType::Aid => SignalType::Aid,
            rootsignal_common::NodeType::Need => SignalType::Need,
            rootsignal_common::NodeType::Notice => SignalType::Notice,
            rootsignal_common::NodeType::Tension => SignalType::Tension,
            rootsignal_common::NodeType::Evidence => SignalType::Notice, // shouldn't happen
        }
    }
}

impl SignalType {
    pub fn to_node_type(self) -> rootsignal_common::NodeType {
        match self {
            SignalType::Gathering => rootsignal_common::NodeType::Gathering,
            SignalType::Aid => rootsignal_common::NodeType::Aid,
            SignalType::Need => rootsignal_common::NodeType::Need,
            SignalType::Notice => rootsignal_common::NodeType::Notice,
            SignalType::Tension => rootsignal_common::NodeType::Tension,
        }
    }
}

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlUrgency {
    Low,
    Medium,
    High,
    Critical,
}

impl From<rootsignal_common::Urgency> for GqlUrgency {
    fn from(u: rootsignal_common::Urgency) -> Self {
        match u {
            rootsignal_common::Urgency::Low => GqlUrgency::Low,
            rootsignal_common::Urgency::Medium => GqlUrgency::Medium,
            rootsignal_common::Urgency::High => GqlUrgency::High,
            rootsignal_common::Urgency::Critical => GqlUrgency::Critical,
        }
    }
}

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl From<rootsignal_common::Severity> for GqlSeverity {
    fn from(s: rootsignal_common::Severity) -> Self {
        match s {
            rootsignal_common::Severity::Low => GqlSeverity::Low,
            rootsignal_common::Severity::Medium => GqlSeverity::Medium,
            rootsignal_common::Severity::High => GqlSeverity::High,
            rootsignal_common::Severity::Critical => GqlSeverity::Critical,
        }
    }
}

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlSensitivityLevel {
    General,
    Elevated,
    Sensitive,
}

impl From<rootsignal_common::SensitivityLevel> for GqlSensitivityLevel {
    fn from(s: rootsignal_common::SensitivityLevel) -> Self {
        match s {
            rootsignal_common::SensitivityLevel::General => GqlSensitivityLevel::General,
            rootsignal_common::SensitivityLevel::Elevated => GqlSensitivityLevel::Elevated,
            rootsignal_common::SensitivityLevel::Sensitive => GqlSensitivityLevel::Sensitive,
        }
    }
}

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlGeoPrecision {
    Exact,
    Neighborhood,
    Approximate,
    Region,
}

impl From<rootsignal_common::GeoPrecision> for GqlGeoPrecision {
    fn from(p: rootsignal_common::GeoPrecision) -> Self {
        match p {
            rootsignal_common::GeoPrecision::Exact => GqlGeoPrecision::Exact,
            rootsignal_common::GeoPrecision::Neighborhood => GqlGeoPrecision::Neighborhood,
            rootsignal_common::GeoPrecision::Approximate => GqlGeoPrecision::Approximate,
            rootsignal_common::GeoPrecision::Region => GqlGeoPrecision::Region,
        }
    }
}

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlActorType {
    Organization,
    Individual,
    GovernmentBody,
    Coalition,
}

impl From<rootsignal_common::ActorType> for GqlActorType {
    fn from(a: rootsignal_common::ActorType) -> Self {
        match a {
            rootsignal_common::ActorType::Organization => GqlActorType::Organization,
            rootsignal_common::ActorType::Individual => GqlActorType::Individual,
            rootsignal_common::ActorType::GovernmentBody => GqlActorType::GovernmentBody,
            rootsignal_common::ActorType::Coalition => GqlActorType::Coalition,
        }
    }
}

// --- GeoPoint ---

pub struct GqlGeoPoint(pub rootsignal_common::GeoPoint);

#[Object]
impl GqlGeoPoint {
    async fn lat(&self) -> f64 {
        self.0.lat
    }
    async fn lng(&self) -> f64 {
        self.0.lng
    }
    async fn precision(&self) -> GqlGeoPrecision {
        self.0.precision.into()
    }
}

// --- Evidence ---

pub struct GqlEvidence(pub EvidenceNode);

#[Object]
impl GqlEvidence {
    async fn id(&self) -> Uuid {
        self.0.id
    }
    async fn source_url(&self) -> &str {
        &self.0.source_url
    }
    async fn retrieved_at(&self) -> DateTime<Utc> {
        self.0.retrieved_at
    }
    async fn content_hash(&self) -> &str {
        &self.0.content_hash
    }
    async fn snippet(&self) -> Option<&str> {
        self.0.snippet.as_deref()
    }
    async fn relevance(&self) -> Option<&str> {
        self.0.relevance.as_deref()
    }
    async fn evidence_confidence(&self) -> Option<f32> {
        self.0.evidence_confidence
    }
}

// --- Signal Union ---

#[derive(Union)]
pub enum GqlSignal {
    Gathering(GqlGatheringSignal),
    Aid(GqlAidSignal),
    Need(GqlNeedSignal),
    Notice(GqlNoticeSignal),
    Tension(GqlTensionSignal),
}

impl From<Node> for GqlSignal {
    fn from(node: Node) -> Self {
        match node {
            Node::Gathering(n) => GqlSignal::Gathering(GqlGatheringSignal(n)),
            Node::Aid(n) => GqlSignal::Aid(GqlAidSignal(n)),
            Node::Need(n) => GqlSignal::Need(GqlNeedSignal(n)),
            Node::Notice(n) => GqlSignal::Notice(GqlNoticeSignal(n)),
            Node::Tension(n) => GqlSignal::Tension(GqlTensionSignal(n)),
            Node::Evidence(_) => unreachable!("Evidence nodes are not signals"),
        }
    }
}

// --- Shared NodeMeta resolver macro ---

/// Generates the shared NodeMeta field resolvers and relationship resolvers for a signal type.
macro_rules! signal_meta_resolvers {
    () => {
        async fn id(&self) -> Uuid {
            self.meta().id
        }
        async fn title(&self) -> &str {
            &self.meta().title
        }
        async fn summary(&self) -> &str {
            &self.meta().summary
        }
        async fn sensitivity(&self) -> GqlSensitivityLevel {
            self.meta().sensitivity.into()
        }
        async fn confidence(&self) -> f32 {
            self.meta().confidence
        }
        async fn location(&self) -> Option<GqlGeoPoint> {
            self.meta().location.map(GqlGeoPoint)
        }
        async fn location_name(&self) -> Option<&str> {
            self.meta().location_name.as_deref()
        }
        async fn source_url(&self) -> &str {
            &self.meta().source_url
        }
        async fn extracted_at(&self) -> DateTime<Utc> {
            self.meta().extracted_at
        }
        async fn content_date(&self) -> Option<DateTime<Utc>> {
            self.meta().content_date
        }
        async fn source_diversity(&self) -> u32 {
            self.meta().source_diversity
        }
        async fn cause_heat(&self) -> f64 {
            self.meta().cause_heat
        }
        async fn channel_diversity(&self) -> u32 {
            self.meta().channel_diversity
        }
        async fn mentioned_actors(&self) -> &[String] {
            &self.meta().mentioned_actors
        }
        async fn evidence(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvidence>> {
            let loader = ctx.data_unchecked::<DataLoader<EvidenceBySignalLoader>>();
            Ok(loader
                .load_one(self.meta().id)
                .await?
                .unwrap_or_default()
                .into_iter()
                .map(GqlEvidence)
                .collect())
        }
        async fn story(&self, ctx: &Context<'_>) -> Result<Option<GqlStory>> {
            let loader = ctx.data_unchecked::<DataLoader<StoryBySignalLoader>>();
            Ok(loader.load_one(self.meta().id).await?.map(GqlStory))
        }
        async fn situations(&self, ctx: &Context<'_>) -> Result<Vec<GqlSituation>> {
            let loader = ctx.data_unchecked::<DataLoader<SituationsBySignalLoader>>();
            Ok(loader
                .load_one(self.meta().id)
                .await?
                .unwrap_or_default()
                .into_iter()
                .map(GqlSituation)
                .collect())
        }
        async fn actors(&self, ctx: &Context<'_>) -> Result<Vec<GqlActor>> {
            let loader = ctx.data_unchecked::<DataLoader<ActorsBySignalLoader>>();
            Ok(loader
                .load_one(self.meta().id)
                .await?
                .unwrap_or_default()
                .into_iter()
                .map(GqlActor)
                .collect())
        }
    };
}

// --- GatheringSignal ---

pub struct GqlGatheringSignal(pub GatheringNode);

impl GqlGatheringSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlGatheringSignal {
    async fn id(&self) -> Uuid { self.meta().id }
    async fn title(&self) -> &str { &self.meta().title }
    async fn summary(&self) -> &str { &self.meta().summary }
    async fn sensitivity(&self) -> GqlSensitivityLevel { self.meta().sensitivity.into() }
    async fn confidence(&self) -> f32 { self.meta().confidence }
    async fn location(&self) -> Option<GqlGeoPoint> { self.meta().location.map(GqlGeoPoint) }
    async fn location_name(&self) -> Option<&str> { self.meta().location_name.as_deref() }
    async fn source_url(&self) -> &str { &self.meta().source_url }
    async fn extracted_at(&self) -> DateTime<Utc> { self.meta().extracted_at }
    async fn content_date(&self) -> Option<DateTime<Utc>> { self.meta().content_date }
    async fn source_diversity(&self) -> u32 { self.meta().source_diversity }
    async fn cause_heat(&self) -> f64 { self.meta().cause_heat }
    async fn channel_diversity(&self) -> u32 { self.meta().channel_diversity }
    async fn mentioned_actors(&self) -> &[String] { &self.meta().mentioned_actors }
    async fn evidence(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvidence>> {
        let loader = ctx.data_unchecked::<DataLoader<EvidenceBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlEvidence).collect())
    }
    async fn story(&self, ctx: &Context<'_>) -> Result<Option<GqlStory>> {
        let loader = ctx.data_unchecked::<DataLoader<StoryBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.map(GqlStory))
    }
    async fn actors(&self, ctx: &Context<'_>) -> Result<Vec<GqlActor>> {
        let loader = ctx.data_unchecked::<DataLoader<ActorsBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlActor).collect())
    }

    async fn starts_at(&self) -> Option<DateTime<Utc>> {
        self.0.starts_at
    }
    async fn ends_at(&self) -> Option<DateTime<Utc>> {
        self.0.ends_at
    }
    async fn action_url(&self) -> &str {
        &self.0.action_url
    }
    async fn organizer(&self) -> Option<&str> {
        self.0.organizer.as_deref()
    }
    async fn is_recurring(&self) -> bool {
        self.0.is_recurring
    }
}

// --- AidSignal ---

pub struct GqlAidSignal(pub AidNode);

impl GqlAidSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlAidSignal {
    async fn id(&self) -> Uuid { self.meta().id }
    async fn title(&self) -> &str { &self.meta().title }
    async fn summary(&self) -> &str { &self.meta().summary }
    async fn sensitivity(&self) -> GqlSensitivityLevel { self.meta().sensitivity.into() }
    async fn confidence(&self) -> f32 { self.meta().confidence }
    async fn location(&self) -> Option<GqlGeoPoint> { self.meta().location.map(GqlGeoPoint) }
    async fn location_name(&self) -> Option<&str> { self.meta().location_name.as_deref() }
    async fn source_url(&self) -> &str { &self.meta().source_url }
    async fn extracted_at(&self) -> DateTime<Utc> { self.meta().extracted_at }
    async fn content_date(&self) -> Option<DateTime<Utc>> { self.meta().content_date }
    async fn source_diversity(&self) -> u32 { self.meta().source_diversity }
    async fn cause_heat(&self) -> f64 { self.meta().cause_heat }
    async fn channel_diversity(&self) -> u32 { self.meta().channel_diversity }
    async fn mentioned_actors(&self) -> &[String] { &self.meta().mentioned_actors }
    async fn evidence(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvidence>> {
        let loader = ctx.data_unchecked::<DataLoader<EvidenceBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlEvidence).collect())
    }
    async fn story(&self, ctx: &Context<'_>) -> Result<Option<GqlStory>> {
        let loader = ctx.data_unchecked::<DataLoader<StoryBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.map(GqlStory))
    }
    async fn actors(&self, ctx: &Context<'_>) -> Result<Vec<GqlActor>> {
        let loader = ctx.data_unchecked::<DataLoader<ActorsBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlActor).collect())
    }

    async fn action_url(&self) -> &str {
        &self.0.action_url
    }
    async fn availability(&self) -> Option<&str> {
        self.0.availability.as_deref()
    }
    async fn is_ongoing(&self) -> bool {
        self.0.is_ongoing
    }
}

// --- NeedSignal ---

pub struct GqlNeedSignal(pub NeedNode);

impl GqlNeedSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlNeedSignal {
    async fn id(&self) -> Uuid { self.meta().id }
    async fn title(&self) -> &str { &self.meta().title }
    async fn summary(&self) -> &str { &self.meta().summary }
    async fn sensitivity(&self) -> GqlSensitivityLevel { self.meta().sensitivity.into() }
    async fn confidence(&self) -> f32 { self.meta().confidence }
    async fn location(&self) -> Option<GqlGeoPoint> { self.meta().location.map(GqlGeoPoint) }
    async fn location_name(&self) -> Option<&str> { self.meta().location_name.as_deref() }
    async fn source_url(&self) -> &str { &self.meta().source_url }
    async fn extracted_at(&self) -> DateTime<Utc> { self.meta().extracted_at }
    async fn content_date(&self) -> Option<DateTime<Utc>> { self.meta().content_date }
    async fn source_diversity(&self) -> u32 { self.meta().source_diversity }
    async fn cause_heat(&self) -> f64 { self.meta().cause_heat }
    async fn channel_diversity(&self) -> u32 { self.meta().channel_diversity }
    async fn mentioned_actors(&self) -> &[String] { &self.meta().mentioned_actors }
    async fn evidence(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvidence>> {
        let loader = ctx.data_unchecked::<DataLoader<EvidenceBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlEvidence).collect())
    }
    async fn story(&self, ctx: &Context<'_>) -> Result<Option<GqlStory>> {
        let loader = ctx.data_unchecked::<DataLoader<StoryBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.map(GqlStory))
    }
    async fn actors(&self, ctx: &Context<'_>) -> Result<Vec<GqlActor>> {
        let loader = ctx.data_unchecked::<DataLoader<ActorsBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlActor).collect())
    }

    async fn urgency(&self) -> GqlUrgency {
        self.0.urgency.into()
    }
    async fn what_needed(&self) -> Option<&str> {
        self.0.what_needed.as_deref()
    }
    async fn action_url(&self) -> Option<&str> {
        self.0.action_url.as_deref()
    }
    async fn goal(&self) -> Option<&str> {
        self.0.goal.as_deref()
    }
}

// --- NoticeSignal ---

pub struct GqlNoticeSignal(pub NoticeNode);

impl GqlNoticeSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlNoticeSignal {
    async fn id(&self) -> Uuid { self.meta().id }
    async fn title(&self) -> &str { &self.meta().title }
    async fn summary(&self) -> &str { &self.meta().summary }
    async fn sensitivity(&self) -> GqlSensitivityLevel { self.meta().sensitivity.into() }
    async fn confidence(&self) -> f32 { self.meta().confidence }
    async fn location(&self) -> Option<GqlGeoPoint> { self.meta().location.map(GqlGeoPoint) }
    async fn location_name(&self) -> Option<&str> { self.meta().location_name.as_deref() }
    async fn source_url(&self) -> &str { &self.meta().source_url }
    async fn extracted_at(&self) -> DateTime<Utc> { self.meta().extracted_at }
    async fn content_date(&self) -> Option<DateTime<Utc>> { self.meta().content_date }
    async fn source_diversity(&self) -> u32 { self.meta().source_diversity }
    async fn cause_heat(&self) -> f64 { self.meta().cause_heat }
    async fn channel_diversity(&self) -> u32 { self.meta().channel_diversity }
    async fn mentioned_actors(&self) -> &[String] { &self.meta().mentioned_actors }
    async fn evidence(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvidence>> {
        let loader = ctx.data_unchecked::<DataLoader<EvidenceBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlEvidence).collect())
    }
    async fn story(&self, ctx: &Context<'_>) -> Result<Option<GqlStory>> {
        let loader = ctx.data_unchecked::<DataLoader<StoryBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.map(GqlStory))
    }
    async fn actors(&self, ctx: &Context<'_>) -> Result<Vec<GqlActor>> {
        let loader = ctx.data_unchecked::<DataLoader<ActorsBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlActor).collect())
    }

    async fn severity(&self) -> GqlSeverity {
        self.0.severity.into()
    }
    async fn category(&self) -> Option<&str> {
        self.0.category.as_deref()
    }
    async fn effective_date(&self) -> Option<DateTime<Utc>> {
        self.0.effective_date
    }
    async fn source_authority(&self) -> Option<&str> {
        self.0.source_authority.as_deref()
    }
}

// --- TensionSignal ---

pub struct GqlTensionSignal(pub TensionNode);

impl GqlTensionSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlTensionSignal {
    async fn id(&self) -> Uuid { self.meta().id }
    async fn title(&self) -> &str { &self.meta().title }
    async fn summary(&self) -> &str { &self.meta().summary }
    async fn sensitivity(&self) -> GqlSensitivityLevel { self.meta().sensitivity.into() }
    async fn confidence(&self) -> f32 { self.meta().confidence }
    async fn location(&self) -> Option<GqlGeoPoint> { self.meta().location.map(GqlGeoPoint) }
    async fn location_name(&self) -> Option<&str> { self.meta().location_name.as_deref() }
    async fn source_url(&self) -> &str { &self.meta().source_url }
    async fn extracted_at(&self) -> DateTime<Utc> { self.meta().extracted_at }
    async fn content_date(&self) -> Option<DateTime<Utc>> { self.meta().content_date }
    async fn source_diversity(&self) -> u32 { self.meta().source_diversity }
    async fn cause_heat(&self) -> f64 { self.meta().cause_heat }
    async fn channel_diversity(&self) -> u32 { self.meta().channel_diversity }
    async fn mentioned_actors(&self) -> &[String] { &self.meta().mentioned_actors }
    async fn evidence(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvidence>> {
        let loader = ctx.data_unchecked::<DataLoader<EvidenceBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlEvidence).collect())
    }
    async fn story(&self, ctx: &Context<'_>) -> Result<Option<GqlStory>> {
        let loader = ctx.data_unchecked::<DataLoader<StoryBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.map(GqlStory))
    }
    async fn actors(&self, ctx: &Context<'_>) -> Result<Vec<GqlActor>> {
        let loader = ctx.data_unchecked::<DataLoader<ActorsBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.unwrap_or_default().into_iter().map(GqlActor).collect())
    }

    async fn severity(&self) -> GqlSeverity {
        self.0.severity.into()
    }
    async fn category(&self) -> Option<&str> {
        self.0.category.as_deref()
    }
    async fn what_would_help(&self) -> Option<&str> {
        self.0.what_would_help.as_deref()
    }
    async fn responses(&self, ctx: &Context<'_>) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let nodes = reader.tension_responses(self.0.meta.id).await?;
        Ok(nodes
            .into_iter()
            .map(|tr| GqlSignal::from(tr.node))
            .collect())
    }
}

// --- Story ---

pub struct GqlStory(pub StoryNode);

#[Object]
impl GqlStory {
    async fn id(&self) -> Uuid {
        self.0.id
    }
    async fn headline(&self) -> &str {
        &self.0.headline
    }
    async fn summary(&self) -> &str {
        &self.0.summary
    }
    async fn signal_count(&self) -> u32 {
        self.0.signal_count
    }
    async fn first_seen(&self) -> DateTime<Utc> {
        self.0.first_seen
    }
    async fn last_updated(&self) -> DateTime<Utc> {
        self.0.last_updated
    }
    async fn velocity(&self) -> f64 {
        self.0.velocity
    }
    async fn energy(&self) -> f64 {
        self.0.energy
    }
    async fn centroid_lat(&self) -> Option<f64> {
        self.0.centroid_lat
    }
    async fn centroid_lng(&self) -> Option<f64> {
        self.0.centroid_lng
    }
    async fn dominant_type(&self) -> &str {
        &self.0.dominant_type
    }
    async fn sensitivity(&self) -> &str {
        &self.0.sensitivity
    }
    async fn source_count(&self) -> u32 {
        self.0.source_count
    }
    async fn entity_count(&self) -> u32 {
        self.0.entity_count
    }
    async fn type_diversity(&self) -> u32 {
        self.0.type_diversity
    }
    async fn source_domains(&self) -> &[String] {
        &self.0.source_domains
    }
    async fn corroboration_depth(&self) -> u32 {
        self.0.corroboration_depth
    }
    async fn status(&self) -> &str {
        &self.0.status
    }
    async fn arc(&self) -> Option<&str> {
        self.0.arc.as_deref()
    }
    async fn category(&self) -> Option<&str> {
        self.0.category.as_deref()
    }
    async fn lede(&self) -> Option<&str> {
        self.0.lede.as_deref()
    }
    async fn narrative(&self) -> Option<&str> {
        self.0.narrative.as_deref()
    }

    async fn cause_heat(&self) -> f64 {
        self.0.cause_heat
    }
    async fn channel_diversity(&self) -> u32 {
        self.0.channel_diversity
    }
    async fn need_count(&self) -> u32 {
        self.0.ask_count
    }
    async fn aid_count(&self) -> u32 {
        self.0.give_count
    }
    async fn gathering_count(&self) -> u32 {
        self.0.event_count
    }
    async fn drawn_to_count(&self) -> u32 {
        self.0.drawn_to_count
    }
    async fn gap_score(&self) -> i32 {
        self.0.gap_score
    }
    async fn gap_velocity(&self) -> f64 {
        self.0.gap_velocity
    }

    async fn signals(&self, ctx: &Context<'_>) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let nodes = reader.get_story_signals(self.0.id).await?;
        Ok(nodes.into_iter().map(GqlSignal::from).collect())
    }

    async fn actors(&self, ctx: &Context<'_>) -> Result<Vec<GqlActor>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let actors = reader.actors_for_story(self.0.id).await?;
        Ok(actors.into_iter().map(GqlActor).collect())
    }

    async fn evidence_count(&self, ctx: &Context<'_>) -> Result<u32> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let counts = reader.story_evidence_counts(&[self.0.id]).await?;
        Ok(counts.into_iter().next().map(|(_, c)| c).unwrap_or(0))
    }

    async fn tags(&self, ctx: &Context<'_>) -> Result<Vec<GqlTag>> {
        let loader = ctx.data_unchecked::<DataLoader<TagsByStoryLoader>>();
        let tags = loader.load_one(self.0.id).await?.unwrap_or_default();
        Ok(tags.into_iter().map(GqlTag).collect())
    }
}

// --- Tag ---

pub struct GqlTag(pub TagNode);

#[Object]
impl GqlTag {
    async fn id(&self) -> Uuid {
        self.0.id
    }
    async fn slug(&self) -> &str {
        &self.0.slug
    }
    async fn name(&self) -> &str {
        &self.0.name
    }
}

// --- Actor ---

pub struct GqlActor(pub ActorNode);

#[Object]
impl GqlActor {
    async fn id(&self) -> Uuid {
        self.0.id
    }
    async fn name(&self) -> &str {
        &self.0.name
    }
    async fn actor_type(&self) -> GqlActorType {
        self.0.actor_type.into()
    }
    async fn entity_id(&self) -> &str {
        &self.0.entity_id
    }
    async fn domains(&self) -> &[String] {
        &self.0.domains
    }
    async fn social_urls(&self) -> &[String] {
        &self.0.social_urls
    }
    async fn description(&self) -> &str {
        &self.0.description
    }
    async fn signal_count(&self) -> u32 {
        self.0.signal_count
    }
    async fn first_seen(&self) -> DateTime<Utc> {
        self.0.first_seen
    }
    async fn last_active(&self) -> DateTime<Utc> {
        self.0.last_active
    }
    async fn typical_roles(&self) -> &[String] {
        &self.0.typical_roles
    }
    async fn location_lat(&self) -> Option<f64> {
        self.0.location_lat
    }
    async fn location_lng(&self) -> Option<f64> {
        self.0.location_lng
    }
    async fn location_name(&self) -> Option<&str> {
        self.0.location_name.as_deref()
    }

    async fn stories(&self, ctx: &Context<'_>) -> Result<Vec<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let stories = reader.actor_stories(self.0.id, 20).await?;
        Ok(stories.into_iter().map(GqlStory).collect())
    }
}

// --- Search Result types (for search app) ---

/// A signal with a blended relevance score from semantic search.
pub struct GqlSearchResult {
    pub signal: GqlSignal,
    pub score: f64,
}

#[Object]
impl GqlSearchResult {
    async fn signal(&self) -> &GqlSignal {
        &self.signal
    }
    async fn score(&self) -> f64 {
        self.score
    }
}

// ========== Supervisor Findings ==========

#[derive(SimpleObject)]
pub struct SupervisorFinding {
    pub id: String,
    pub issue_type: String,
    pub severity: String,
    pub target_id: String,
    pub target_label: String,
    pub description: String,
    pub suggested_action: String,
    pub status: String,
    pub created_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(SimpleObject)]
pub struct SupervisorSummary {
    pub total_open: i64,
    pub total_resolved: i64,
    pub total_dismissed: i64,
    pub count_by_type: Vec<FindingCount>,
    pub count_by_severity: Vec<FindingCount>,
}

#[derive(SimpleObject)]
pub struct FindingCount {
    pub label: String,
    pub count: i64,
}

/// A story matched via its constituent signals' semantic similarity.
pub struct GqlStorySearchResult {
    pub story: GqlStory,
    pub score: f64,
    pub top_matching_signal_title: Option<String>,
}

#[Object]
impl GqlStorySearchResult {
    async fn story(&self) -> &GqlStory {
        &self.story
    }
    async fn score(&self) -> f64 {
        self.score
    }
    async fn top_matching_signal_title(&self) -> Option<&str> {
        self.top_matching_signal_title.as_deref()
    }
}

// --- Situation types ---

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlSituationArc {
    Emerging,
    Developing,
    Active,
    Cooling,
    Cold,
}

impl From<rootsignal_common::SituationArc> for GqlSituationArc {
    fn from(a: rootsignal_common::SituationArc) -> Self {
        match a {
            rootsignal_common::SituationArc::Emerging => GqlSituationArc::Emerging,
            rootsignal_common::SituationArc::Developing => GqlSituationArc::Developing,
            rootsignal_common::SituationArc::Active => GqlSituationArc::Active,
            rootsignal_common::SituationArc::Cooling => GqlSituationArc::Cooling,
            rootsignal_common::SituationArc::Cold => GqlSituationArc::Cold,
        }
    }
}

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlClarity {
    Fuzzy,
    Sharpening,
    Sharp,
}

impl From<rootsignal_common::Clarity> for GqlClarity {
    fn from(c: rootsignal_common::Clarity) -> Self {
        match c {
            rootsignal_common::Clarity::Fuzzy => GqlClarity::Fuzzy,
            rootsignal_common::Clarity::Sharpening => GqlClarity::Sharpening,
            rootsignal_common::Clarity::Sharp => GqlClarity::Sharp,
        }
    }
}

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlDispatchType {
    Update,
    Emergence,
    Split,
    Merge,
    Reactivation,
    Correction,
}

impl From<rootsignal_common::DispatchType> for GqlDispatchType {
    fn from(d: rootsignal_common::DispatchType) -> Self {
        match d {
            rootsignal_common::DispatchType::Update => GqlDispatchType::Update,
            rootsignal_common::DispatchType::Emergence => GqlDispatchType::Emergence,
            rootsignal_common::DispatchType::Split => GqlDispatchType::Split,
            rootsignal_common::DispatchType::Merge => GqlDispatchType::Merge,
            rootsignal_common::DispatchType::Reactivation => GqlDispatchType::Reactivation,
            rootsignal_common::DispatchType::Correction => GqlDispatchType::Correction,
        }
    }
}

pub struct GqlSituation(pub rootsignal_common::SituationNode);

#[Object]
impl GqlSituation {
    async fn id(&self) -> Uuid {
        self.0.id
    }
    async fn headline(&self) -> &str {
        &self.0.headline
    }
    async fn lede(&self) -> &str {
        &self.0.lede
    }
    async fn arc(&self) -> GqlSituationArc {
        self.0.arc.into()
    }
    async fn temperature(&self) -> f64 {
        self.0.temperature
    }
    async fn tension_heat(&self) -> f64 {
        self.0.tension_heat
    }
    async fn entity_velocity(&self) -> f64 {
        self.0.entity_velocity
    }
    async fn amplification(&self) -> f64 {
        self.0.amplification
    }
    async fn response_coverage(&self) -> f64 {
        self.0.response_coverage
    }
    async fn clarity_need(&self) -> f64 {
        self.0.clarity_need
    }
    async fn clarity(&self) -> GqlClarity {
        self.0.clarity.into()
    }
    async fn centroid_lat(&self) -> Option<f64> {
        self.0.centroid_lat
    }
    async fn centroid_lng(&self) -> Option<f64> {
        self.0.centroid_lng
    }
    async fn location_name(&self) -> Option<&str> {
        self.0.location_name.as_deref()
    }
    async fn signal_count(&self) -> u32 {
        self.0.signal_count
    }
    async fn tension_count(&self) -> u32 {
        self.0.tension_count
    }
    async fn dispatch_count(&self) -> u32 {
        self.0.dispatch_count
    }
    async fn first_seen(&self) -> DateTime<Utc> {
        self.0.first_seen
    }
    async fn last_updated(&self) -> DateTime<Utc> {
        self.0.last_updated
    }
    async fn sensitivity(&self) -> &str {
        self.0.sensitivity.as_str()
    }
    async fn category(&self) -> Option<&str> {
        self.0.category.as_deref()
    }

    async fn tags(&self, ctx: &Context<'_>) -> Result<Vec<GqlTag>> {
        let loader = ctx.data_unchecked::<DataLoader<TagsBySituationLoader>>();
        let tags = loader.load_one(self.0.id).await?.unwrap_or_default();
        Ok(tags.into_iter().map(GqlTag).collect())
    }

    /// Dispatches for this situation, ordered chronologically.
    async fn dispatches(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 20)] limit: u32,
        #[graphql(default = 0)] offset: u32,
    ) -> Result<Vec<GqlDispatch>> {
        let client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();
        let reader = rootsignal_graph::PublicGraphReader::new(client.as_ref().clone());
        let dispatches = reader
            .dispatches_for_situation(&self.0.id, limit.min(100), offset)
            .await?;
        Ok(dispatches.into_iter().map(GqlDispatch).collect())
    }
}

pub struct GqlDispatch(pub rootsignal_common::DispatchNode);

#[Object]
impl GqlDispatch {
    async fn id(&self) -> Uuid {
        self.0.id
    }
    async fn situation_id(&self) -> Uuid {
        self.0.situation_id
    }
    async fn body(&self) -> &str {
        &self.0.body
    }
    async fn signal_ids(&self) -> Vec<String> {
        self.0.signal_ids.iter().map(|id| id.to_string()).collect()
    }
    async fn created_at(&self) -> DateTime<Utc> {
        self.0.created_at
    }
    async fn dispatch_type(&self) -> GqlDispatchType {
        self.0.dispatch_type.into()
    }
    async fn supersedes(&self) -> Option<Uuid> {
        self.0.supersedes
    }
    async fn flagged_for_review(&self) -> bool {
        self.0.flagged_for_review
    }
    async fn flag_reason(&self) -> Option<&str> {
        self.0.flag_reason.as_deref()
    }
    async fn fidelity_score(&self) -> Option<f64> {
        self.0.fidelity_score
    }
}

// --- Scout Task types ---

#[derive(SimpleObject)]
pub struct GqlScoutTask {
    pub id: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub context: String,
    pub geo_terms: Vec<String>,
    pub priority: f64,
    pub source: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    /// Current workflow phase status for this task's region (e.g. "complete", "idle", "running_scrape").
    pub phase_status: String,
}

impl GqlScoutTask {
    pub fn from_task(t: rootsignal_common::ScoutTask) -> Self {
        GqlScoutTask {
            id: t.id.to_string(),
            center_lat: t.center_lat,
            center_lng: t.center_lng,
            radius_km: t.radius_km,
            context: t.context.clone(),
            geo_terms: t.geo_terms,
            priority: t.priority,
            source: t.source.to_string(),
            status: t.status.to_string(),
            created_at: t.created_at.to_rfc3339(),
            completed_at: t.completed_at.map(|dt| dt.to_rfc3339()),
            phase_status: t.phase_status,
        }
    }
}
