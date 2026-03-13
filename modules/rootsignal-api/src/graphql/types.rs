// async-graphql's #[Object] proc macro transforms methods into trait impls,
// causing false-positive dead_code warnings on helpers called from macro-expanded resolvers.
#![allow(dead_code)]

use std::sync::Arc;
use async_graphql::dataloader::DataLoader;
use async_graphql::{Context, Object, Result, SimpleObject, Union};
use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono::TimeZone;
use uuid::Uuid;
use rootsignal_common::{
ActorNode, ConditionNode, ResourceOfferNode, CitationNode, GatheringNode, HelpRequestNode,
Node, NodeMeta, AnnouncementNode, ScheduleNode, TagNode, ConcernNode,
};
use rootsignal_graph::CachedReader;
use super::loaders::{
ActorsBySignalLoader, CitationBySignalLoader, ScheduleBySignalLoader, TagsBySituationLoader,
};


// --- GraphQL Enums ---

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum SignalType {
    Gathering,
    Resource,
    HelpRequest,
    Announcement,
    Concern,
    Condition,
}

impl From<rootsignal_common::NodeType> for SignalType {
    fn from(nt: rootsignal_common::NodeType) -> Self {
        match nt {
            rootsignal_common::NodeType::Gathering => SignalType::Gathering,
            rootsignal_common::NodeType::Resource => SignalType::Resource,
            rootsignal_common::NodeType::HelpRequest => SignalType::HelpRequest,
            rootsignal_common::NodeType::Announcement => SignalType::Announcement,
            rootsignal_common::NodeType::Concern => SignalType::Concern,
            rootsignal_common::NodeType::Condition => SignalType::Condition,
            rootsignal_common::NodeType::Citation => SignalType::Announcement, // shouldn't happen
        }
    }
}

impl SignalType {
    pub fn to_node_type(self) -> rootsignal_common::NodeType {
        match self {
            SignalType::Gathering => rootsignal_common::NodeType::Gathering,
            SignalType::Resource => rootsignal_common::NodeType::Resource,
            SignalType::HelpRequest => rootsignal_common::NodeType::HelpRequest,
            SignalType::Announcement => rootsignal_common::NodeType::Announcement,
            SignalType::Concern => rootsignal_common::NodeType::Concern,
            SignalType::Condition => rootsignal_common::NodeType::Condition,
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

// --- Citation ---

pub struct GqlCitation(pub CitationNode);

#[Object]
impl GqlCitation {
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
        self.0.confidence
    }
}

// --- Schedule ---

pub struct GqlSchedule(pub ScheduleNode);

#[Object]
impl GqlSchedule {
    async fn id(&self) -> Uuid {
        self.0.id
    }
    async fn rrule(&self) -> Option<&str> {
        self.0.rrule.as_deref()
    }
    async fn rdates(&self) -> &[DateTime<Utc>] {
        &self.0.rdates
    }
    async fn exdates(&self) -> &[DateTime<Utc>] {
        &self.0.exdates
    }
    async fn dtstart(&self) -> Option<DateTime<Utc>> {
        self.0.dtstart
    }
    async fn dtend(&self) -> Option<DateTime<Utc>> {
        self.0.dtend
    }
    async fn timezone(&self) -> Option<&str> {
        self.0.timezone.as_deref()
    }
    async fn schedule_text(&self) -> Option<&str> {
        self.0.schedule_text.as_deref()
    }
    async fn extracted_at(&self) -> DateTime<Utc> {
        self.0.extracted_at
    }

    /// Expand the RRULE into concrete occurrence datetimes within a window.
    /// Falls back to rdates if no rrule is present. Returns empty if text-only.
    async fn occurrences(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>> {

        // Cap window at 365 days
        let max_window = chrono::Duration::days(365);
        let to = if to - from > max_window {
            from + max_window
        } else {
            to
        };

        if let Some(ref rule) = self.0.rrule {
            let dtstart_str = self
                .0
                .dtstart
                .map(|dt| dt.format("%Y%m%dT%H%M%SZ").to_string())
                .unwrap_or_else(|| "20260101T000000Z".to_string());
            let rrule_str = format!("DTSTART:{dtstart_str}\nRRULE:{rule}");

            let rrule_set: rrule::RRuleSet = rrule_str
                .parse()
                .map_err(|e| async_graphql::Error::new(format!("Invalid RRULE: {e}")))?;

            let tz_from = rrule::Tz::UTC
                .with_ymd_and_hms(
                    from.year(),
                    from.month(),
                    from.day(),
                    from.hour(),
                    from.minute(),
                    from.second(),
                )
                .single()
                .ok_or_else(|| async_graphql::Error::new("Invalid from date"))?;

            let tz_to = rrule::Tz::UTC
                .with_ymd_and_hms(
                    to.year(),
                    to.month(),
                    to.day(),
                    to.hour(),
                    to.minute(),
                    to.second(),
                )
                .single()
                .ok_or_else(|| async_graphql::Error::new("Invalid to date"))?;

            let result = rrule_set.after(tz_from).before(tz_to).all(1000);
            let dates: Vec<DateTime<Utc>> = result
                .dates
                .into_iter()
                .map(|dt| Utc.timestamp_opt(dt.timestamp(), 0).unwrap())
                .collect();
            Ok(dates)
        } else if !self.0.rdates.is_empty() {
            // No RRULE — return rdates within the window, minus exdates
            let dates: Vec<DateTime<Utc>> = self
                .0
                .rdates
                .iter()
                .filter(|d| **d >= from && **d <= to)
                .filter(|d| !self.0.exdates.contains(d))
                .copied()
                .collect();
            Ok(dates)
        } else {
            // Text-only schedule — no expansion possible
            Ok(Vec::new())
        }
    }
}

// --- Signal Union ---

#[derive(Union)]
pub enum GqlSignal {
    Gathering(GqlGatheringSignal),
    Resource(GqlResourceSignal),
    HelpRequest(GqlHelpRequestSignal),
    Announcement(GqlAnnouncementSignal),
    Concern(GqlConcernSignal),
    Condition(GqlConditionSignal),
}

impl From<Node> for GqlSignal {
    fn from(node: Node) -> Self {
        match node {
            Node::Gathering(n) => GqlSignal::Gathering(GqlGatheringSignal(n)),
            Node::Resource(n) => GqlSignal::Resource(GqlResourceSignal(n)),
            Node::HelpRequest(n) => GqlSignal::HelpRequest(GqlHelpRequestSignal(n)),
            Node::Announcement(n) => GqlSignal::Announcement(GqlAnnouncementSignal(n)),
            Node::Concern(n) => GqlSignal::Concern(GqlConcernSignal(n)),
            Node::Condition(n) => GqlSignal::Condition(GqlConditionSignal(n)),
            Node::Citation(_) => unreachable!("Citation nodes are not signals"),
        }
    }
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
        self.meta().about_point().copied().map(GqlGeoPoint)
    }
    async fn location_name(&self) -> Option<&str> {
        self.meta().about_location_name()
    }
    async fn url(&self) -> &str {
        &self.meta().url
    }
    async fn extracted_at(&self) -> DateTime<Utc> {
        self.meta().extracted_at
    }
    async fn published_at(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
    }
    async fn content_date(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
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
    async fn review_status(&self) -> &'static str {
        match self.meta().review_status {
            rootsignal_common::ReviewStatus::Staged => "staged",
            rootsignal_common::ReviewStatus::Accepted => "accepted",
            rootsignal_common::ReviewStatus::Rejected => "rejected",
            rootsignal_common::ReviewStatus::Corrected => "corrected",
        }
    }
    async fn was_corrected(&self) -> bool {
        self.meta().was_corrected
    }
    async fn corrections(&self) -> Option<&str> {
        self.meta().corrections.as_deref()
    }
    async fn rejection_reason(&self) -> Option<&str> {
        self.meta().rejection_reason.as_deref()
    }
    async fn category(&self) -> Option<&str> {
        self.meta().category.as_deref()
    }
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let loader = ctx.data_unchecked::<DataLoader<CitationBySignalLoader>>();
        Ok(loader
            .load_one(self.meta().id)
            .await?
            .unwrap_or_default()
            .into_iter()
            .map(GqlCitation)
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
    async fn schedule(&self, ctx: &Context<'_>) -> Result<Option<GqlSchedule>> {
        let loader = ctx.data_unchecked::<DataLoader<ScheduleBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.map(GqlSchedule))
    }
}

// --- AidSignal ---

pub struct GqlResourceSignal(pub ResourceOfferNode);

impl GqlResourceSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlResourceSignal {
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
        self.meta().about_point().copied().map(GqlGeoPoint)
    }
    async fn location_name(&self) -> Option<&str> {
        self.meta().about_location_name()
    }
    async fn url(&self) -> &str {
        &self.meta().url
    }
    async fn extracted_at(&self) -> DateTime<Utc> {
        self.meta().extracted_at
    }
    async fn published_at(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
    }
    async fn content_date(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
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
    async fn review_status(&self) -> &'static str {
        match self.meta().review_status {
            rootsignal_common::ReviewStatus::Staged => "staged",
            rootsignal_common::ReviewStatus::Accepted => "accepted",
            rootsignal_common::ReviewStatus::Rejected => "rejected",
            rootsignal_common::ReviewStatus::Corrected => "corrected",
        }
    }
    async fn was_corrected(&self) -> bool {
        self.meta().was_corrected
    }
    async fn corrections(&self) -> Option<&str> {
        self.meta().corrections.as_deref()
    }
    async fn rejection_reason(&self) -> Option<&str> {
        self.meta().rejection_reason.as_deref()
    }
    async fn category(&self) -> Option<&str> {
        self.meta().category.as_deref()
    }
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let loader = ctx.data_unchecked::<DataLoader<CitationBySignalLoader>>();
        Ok(loader
            .load_one(self.meta().id)
            .await?
            .unwrap_or_default()
            .into_iter()
            .map(GqlCitation)
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

    async fn action_url(&self) -> &str {
        &self.0.action_url
    }
    async fn availability(&self) -> Option<&str> {
        self.0.availability.as_deref()
    }
    async fn is_ongoing(&self) -> bool {
        self.0.is_ongoing
    }
    async fn schedule(&self, ctx: &Context<'_>) -> Result<Option<GqlSchedule>> {
        let loader = ctx.data_unchecked::<DataLoader<ScheduleBySignalLoader>>();
        Ok(loader.load_one(self.meta().id).await?.map(GqlSchedule))
    }
}

// --- NeedSignal ---

pub struct GqlHelpRequestSignal(pub HelpRequestNode);

impl GqlHelpRequestSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlHelpRequestSignal {
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
        self.meta().about_point().copied().map(GqlGeoPoint)
    }
    async fn location_name(&self) -> Option<&str> {
        self.meta().about_location_name()
    }
    async fn url(&self) -> &str {
        &self.meta().url
    }
    async fn extracted_at(&self) -> DateTime<Utc> {
        self.meta().extracted_at
    }
    async fn published_at(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
    }
    async fn content_date(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
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
    async fn review_status(&self) -> &'static str {
        match self.meta().review_status {
            rootsignal_common::ReviewStatus::Staged => "staged",
            rootsignal_common::ReviewStatus::Accepted => "accepted",
            rootsignal_common::ReviewStatus::Rejected => "rejected",
            rootsignal_common::ReviewStatus::Corrected => "corrected",
        }
    }
    async fn was_corrected(&self) -> bool {
        self.meta().was_corrected
    }
    async fn corrections(&self) -> Option<&str> {
        self.meta().corrections.as_deref()
    }
    async fn rejection_reason(&self) -> Option<&str> {
        self.meta().rejection_reason.as_deref()
    }
    async fn category(&self) -> Option<&str> {
        self.meta().category.as_deref()
    }
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let loader = ctx.data_unchecked::<DataLoader<CitationBySignalLoader>>();
        Ok(loader
            .load_one(self.meta().id)
            .await?
            .unwrap_or_default()
            .into_iter()
            .map(GqlCitation)
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

    async fn urgency(&self) -> GqlUrgency {
        self.0.urgency.into()
    }
    async fn what_needed(&self) -> Option<&str> {
        self.0.what_needed.as_deref()
    }
    async fn action_url(&self) -> Option<&str> {
        self.0.action_url.as_deref()
    }
    async fn stated_goal(&self) -> Option<&str> {
        self.0.stated_goal.as_deref()
    }
}

// --- NoticeSignal ---

pub struct GqlAnnouncementSignal(pub AnnouncementNode);

impl GqlAnnouncementSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlAnnouncementSignal {
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
        self.meta().about_point().copied().map(GqlGeoPoint)
    }
    async fn location_name(&self) -> Option<&str> {
        self.meta().about_location_name()
    }
    async fn url(&self) -> &str {
        &self.meta().url
    }
    async fn extracted_at(&self) -> DateTime<Utc> {
        self.meta().extracted_at
    }
    async fn published_at(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
    }
    async fn content_date(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
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
    async fn review_status(&self) -> &'static str {
        match self.meta().review_status {
            rootsignal_common::ReviewStatus::Staged => "staged",
            rootsignal_common::ReviewStatus::Accepted => "accepted",
            rootsignal_common::ReviewStatus::Rejected => "rejected",
            rootsignal_common::ReviewStatus::Corrected => "corrected",
        }
    }
    async fn was_corrected(&self) -> bool {
        self.meta().was_corrected
    }
    async fn corrections(&self) -> Option<&str> {
        self.meta().corrections.as_deref()
    }
    async fn rejection_reason(&self) -> Option<&str> {
        self.meta().rejection_reason.as_deref()
    }
    async fn category(&self) -> Option<&str> {
        self.meta().category.as_deref()
    }
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let loader = ctx.data_unchecked::<DataLoader<CitationBySignalLoader>>();
        Ok(loader
            .load_one(self.meta().id)
            .await?
            .unwrap_or_default()
            .into_iter()
            .map(GqlCitation)
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

    async fn severity(&self) -> GqlSeverity {
        self.0.severity.into()
    }
    async fn subject(&self) -> Option<&str> {
        self.0.subject.as_deref()
    }
    async fn effective_date(&self) -> Option<DateTime<Utc>> {
        self.0.effective_date
    }
    async fn source_authority(&self) -> Option<&str> {
        self.0.source_authority.as_deref()
    }
}

// --- ConcernSignal ---

pub struct GqlConcernSignal(pub ConcernNode);

impl GqlConcernSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlConcernSignal {
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
        self.meta().about_point().copied().map(GqlGeoPoint)
    }
    async fn location_name(&self) -> Option<&str> {
        self.meta().about_location_name()
    }
    async fn url(&self) -> &str {
        &self.meta().url
    }
    async fn extracted_at(&self) -> DateTime<Utc> {
        self.meta().extracted_at
    }
    async fn published_at(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
    }
    async fn content_date(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
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
    async fn review_status(&self) -> &'static str {
        match self.meta().review_status {
            rootsignal_common::ReviewStatus::Staged => "staged",
            rootsignal_common::ReviewStatus::Accepted => "accepted",
            rootsignal_common::ReviewStatus::Rejected => "rejected",
            rootsignal_common::ReviewStatus::Corrected => "corrected",
        }
    }
    async fn was_corrected(&self) -> bool {
        self.meta().was_corrected
    }
    async fn corrections(&self) -> Option<&str> {
        self.meta().corrections.as_deref()
    }
    async fn rejection_reason(&self) -> Option<&str> {
        self.meta().rejection_reason.as_deref()
    }
    async fn category(&self) -> Option<&str> {
        self.meta().category.as_deref()
    }
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let loader = ctx.data_unchecked::<DataLoader<CitationBySignalLoader>>();
        Ok(loader
            .load_one(self.meta().id)
            .await?
            .unwrap_or_default()
            .into_iter()
            .map(GqlCitation)
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

    async fn severity(&self) -> GqlSeverity {
        self.0.severity.into()
    }
    async fn subject(&self) -> Option<&str> {
        self.0.subject.as_deref()
    }
    async fn opposing(&self) -> Option<&str> {
        self.0.opposing.as_deref()
    }
    async fn responses(&self, ctx: &Context<'_>) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let nodes = reader.concern_responses(self.0.meta.id).await?;
        Ok(nodes
            .into_iter()
            .map(|tr| GqlSignal::from(tr.node))
            .collect())
    }
}

// --- ConditionSignal ---

pub struct GqlConditionSignal(pub ConditionNode);

impl GqlConditionSignal {
    fn meta(&self) -> &NodeMeta {
        &self.0.meta
    }
}

#[Object]
impl GqlConditionSignal {
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
        self.meta().about_point().copied().map(GqlGeoPoint)
    }
    async fn location_name(&self) -> Option<&str> {
        self.meta().about_location_name()
    }
    async fn url(&self) -> &str {
        &self.meta().url
    }
    async fn extracted_at(&self) -> DateTime<Utc> {
        self.meta().extracted_at
    }
    async fn published_at(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
    }
    async fn content_date(&self) -> Option<DateTime<Utc>> {
        self.meta().published_at
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
    async fn review_status(&self) -> &'static str {
        match self.meta().review_status {
            rootsignal_common::ReviewStatus::Staged => "staged",
            rootsignal_common::ReviewStatus::Accepted => "accepted",
            rootsignal_common::ReviewStatus::Rejected => "rejected",
            rootsignal_common::ReviewStatus::Corrected => "corrected",
        }
    }
    async fn was_corrected(&self) -> bool {
        self.meta().was_corrected
    }
    async fn corrections(&self) -> Option<&str> {
        self.meta().corrections.as_deref()
    }
    async fn rejection_reason(&self) -> Option<&str> {
        self.meta().rejection_reason.as_deref()
    }
    async fn category(&self) -> Option<&str> {
        self.meta().category.as_deref()
    }
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let loader = ctx.data_unchecked::<DataLoader<CitationBySignalLoader>>();
        Ok(loader
            .load_one(self.meta().id)
            .await?
            .unwrap_or_default()
            .into_iter()
            .map(GqlCitation)
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

    async fn severity(&self) -> GqlSeverity {
        self.0.severity.into()
    }
    async fn subject(&self) -> Option<&str> {
        self.0.subject.as_deref()
    }
    async fn observed_by(&self) -> Option<&str> {
        self.0.observed_by.as_deref()
    }
    async fn measurement(&self) -> Option<&str> {
        self.0.measurement.as_deref()
    }
    async fn affected_scope(&self) -> Option<&str> {
        self.0.affected_scope.as_deref()
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
    async fn canonical_key(&self) -> &str {
        &self.0.canonical_key
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

// --- Scheduled scrapes ---

#[derive(SimpleObject)]
pub struct ScheduledScrape {
    pub id: String,
    pub scope_type: String,
    pub scope_data: String,
    pub run_after: DateTime<Utc>,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl From<crate::db::scheduled_scrapes::ScheduledScrapeRow> for ScheduledScrape {
    fn from(row: crate::db::scheduled_scrapes::ScheduledScrapeRow) -> Self {
        Self {
            id: row.id,
            scope_type: row.scope_type,
            scope_data: row.scope_data.to_string(),
            run_after: row.run_after,
            reason: row.reason,
            created_at: row.created_at,
            completed_at: row.completed_at,
        }
    }
}

// --- Schedules ---

#[derive(SimpleObject)]
pub struct Schedule {
    pub schedule_id: String,
    pub flow_type: String,
    pub scope: String,
    pub timeout: i32,
    pub base_timeout: i32,
    pub recurring: bool,
    pub enabled: bool,
    pub last_run_id: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub region_id: Option<String>,
}

impl From<crate::db::models::schedule::ScheduleRow> for Schedule {
    fn from(row: crate::db::models::schedule::ScheduleRow) -> Self {
        Self {
            schedule_id: row.schedule_id,
            flow_type: row.flow_type,
            scope: row.scope.to_string(),
            timeout: row.timeout,
            base_timeout: row.base_timeout,
            recurring: row.recurring,
            enabled: row.enabled,
            last_run_id: row.last_run_id,
            next_run_at: row.next_run_at,
            deleted_at: row.deleted_at,
            created_at: row.created_at,
            region_id: row.region_id,
        }
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
    async fn briefing_body(&self) -> Option<&str> {
        self.0.briefing_body.as_deref()
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

    /// Member signals assigned to this situation (via PART_OF edges).
    async fn signals(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 50)] limit: u32,
    ) -> Result<Vec<GqlSignal>> {
        let client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();
        let reader = rootsignal_graph::PublicGraphReader::new(client.as_ref().clone());
        let nodes = reader
            .signals_for_situation(&self.0.id, limit.min(100))
            .await?;
        Ok(nodes.into_iter().map(GqlSignal::from).collect())
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

// --- Region types ---

#[derive(SimpleObject)]
pub struct GqlRegion {
    pub id: String,
    pub name: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub geo_terms: Vec<String>,
    pub is_leaf: bool,
    pub created_at: String,
}

impl GqlRegion {
    pub fn from_region(r: rootsignal_common::Region) -> Self {
        GqlRegion {
            id: r.id.to_string(),
            name: r.name,
            center_lat: r.center_lat,
            center_lng: r.center_lng,
            radius_km: r.radius_km,
            geo_terms: r.geo_terms,
            is_leaf: r.is_leaf,
            created_at: r.created_at.to_rfc3339(),
        }
    }
}

/// GraphQL enum for flow types.
#[derive(async_graphql::Enum, Debug, Copy, Clone, Eq, PartialEq)]
pub enum GqlFlowType {
    Bootstrap,
    Scrape,
    Weave,
    ScoutSource,
}

// --- Graph Explorer types ---

#[derive(SimpleObject)]
pub struct GqlGraphNeighborhood {
    pub nodes: Vec<GqlGraphNode>,
    pub edges: Vec<GqlGraphEdge>,
    pub total_count: u32,
}

#[derive(SimpleObject)]
pub struct GqlGraphNode {
    pub id: String,
    pub node_type: String,
    pub label: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub confidence: Option<f64>,
    pub metadata: String,
}

#[derive(SimpleObject)]
pub struct GqlGraphEdge {
    pub source_id: String,
    pub target_id: String,
    pub edge_type: String,
}


