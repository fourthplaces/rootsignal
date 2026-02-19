// async-graphql's #[Object] proc macro transforms methods into trait impls,
// causing false-positive dead_code warnings on helpers called from macro-expanded resolvers.
#![allow(dead_code)]

use std::sync::Arc;

use async_graphql::dataloader::DataLoader;
use async_graphql::{Context, Object, Result, Union};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::{
    ActorNode, AskNode, EvidenceNode, EventNode, GiveNode, NodeMeta, NoticeNode,
    StoryNode, TensionNode, Node,
};
use rootsignal_graph::PublicGraphReader;

use super::loaders::{ActorsBySignalLoader, EvidenceBySignalLoader, StoryBySignalLoader};

// --- GraphQL Enums ---

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum SignalType {
    Event,
    Give,
    Ask,
    Notice,
    Tension,
}

impl From<rootsignal_common::NodeType> for SignalType {
    fn from(nt: rootsignal_common::NodeType) -> Self {
        match nt {
            rootsignal_common::NodeType::Event => SignalType::Event,
            rootsignal_common::NodeType::Give => SignalType::Give,
            rootsignal_common::NodeType::Ask => SignalType::Ask,
            rootsignal_common::NodeType::Notice => SignalType::Notice,
            rootsignal_common::NodeType::Tension => SignalType::Tension,
            rootsignal_common::NodeType::Evidence => SignalType::Notice, // shouldn't happen
        }
    }
}

impl SignalType {
    pub fn to_node_type(self) -> rootsignal_common::NodeType {
        match self {
            SignalType::Event => rootsignal_common::NodeType::Event,
            SignalType::Give => rootsignal_common::NodeType::Give,
            SignalType::Ask => rootsignal_common::NodeType::Ask,
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
    City,
    Region,
}

impl From<rootsignal_common::GeoPrecision> for GqlGeoPrecision {
    fn from(p: rootsignal_common::GeoPrecision) -> Self {
        match p {
            rootsignal_common::GeoPrecision::Exact => GqlGeoPrecision::Exact,
            rootsignal_common::GeoPrecision::Neighborhood => GqlGeoPrecision::Neighborhood,
            rootsignal_common::GeoPrecision::City => GqlGeoPrecision::City,
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
    Event(GqlEventSignal),
    Give(GqlGiveSignal),
    Ask(GqlAskSignal),
    Notice(GqlNoticeSignal),
    Tension(GqlTensionSignal),
}

impl From<Node> for GqlSignal {
    fn from(node: Node) -> Self {
        match node {
            Node::Event(n) => GqlSignal::Event(GqlEventSignal(n)),
            Node::Give(n) => GqlSignal::Give(GqlGiveSignal(n)),
            Node::Ask(n) => GqlSignal::Ask(GqlAskSignal(n)),
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
        async fn source_diversity(&self) -> u32 {
            self.meta().source_diversity
        }
        async fn cause_heat(&self) -> f64 {
            self.meta().cause_heat
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

// --- EventSignal ---

pub struct GqlEventSignal(pub EventNode);

impl GqlEventSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlEventSignal {
    signal_meta_resolvers!();

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

// --- GiveSignal ---

pub struct GqlGiveSignal(pub GiveNode);

impl GqlGiveSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlGiveSignal {
    signal_meta_resolvers!();

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

// --- AskSignal ---

pub struct GqlAskSignal(pub AskNode);

impl GqlAskSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlAskSignal {
    signal_meta_resolvers!();

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
    signal_meta_resolvers!();

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
    signal_meta_resolvers!();

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
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let nodes = reader.tension_responses(self.0.meta.id).await?;
        Ok(nodes.into_iter().map(|tr| GqlSignal::from(tr.node)).collect())
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

    async fn signals(&self, ctx: &Context<'_>) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let nodes = reader.get_story_signals(self.0.id).await?;
        Ok(nodes.into_iter().map(GqlSignal::from).collect())
    }

    async fn actors(&self, ctx: &Context<'_>) -> Result<Vec<GqlActor>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let actors = reader.actors_for_story(self.0.id).await?;
        Ok(actors.into_iter().map(GqlActor).collect())
    }

    async fn evidence_count(&self, ctx: &Context<'_>) -> Result<u32> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let counts = reader.story_evidence_counts(&[self.0.id]).await?;
        Ok(counts.into_iter().next().map(|(_, c)| c).unwrap_or(0))
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
    async fn city(&self) -> &str {
        &self.0.city
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

    async fn stories(&self, ctx: &Context<'_>) -> Result<Vec<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let stories = reader.actor_stories(self.0.id, 20).await?;
        Ok(stories.into_iter().map(GqlStory).collect())
    }
}

