//! Unified event enum — facts about what happened in the world.
//!
//! Every variant describes something the system observed or decided.
//! No embeddings, no derived metrics, no infrastructure artifacts.
//! Events serialize to `serde_json::Value` for the generic EventStore.
//!
//! Naming convention: events describe observations, not domain concepts.
//! "Signal" is graph language — events don't know about it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::safety::SensitivityLevel;
use crate::types::{
    ActorType, ChannelType, DiscoveryMethod, DispatchType, GeoPoint, NodeType, Severity,
    SituationArc, SourceRole, Urgency,
};

// ---------------------------------------------------------------------------
// Value types — reusable structs for event payloads
// ---------------------------------------------------------------------------

/// When something happens. Enough to put it on a calendar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    /// Start of the first/next occurrence (None = unknown)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<DateTime<Utc>>,
    /// End of the occurrence (None = open-ended or unknown)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<DateTime<Utc>>,
    /// True if this is a whole-day event (ignore time component of starts_at/ends_at)
    #[serde(default)]
    pub all_day: bool,
    /// RFC 5545 recurrence rule (e.g. "FREQ=WEEKLY;BYDAY=SA")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rrule: Option<String>,
    /// IANA timezone (e.g. "America/Chicago") for local time rendering
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// Where something is. Enough to put it on a map and give directions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// Coordinates with precision level
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<GeoPoint>,
    /// Human-readable name (e.g. "Lake Harriet Bandshell")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Street address if known
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

/// A tag with its computed weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagFact {
    pub slug: String,
    pub name: String,
    pub weight: f64,
}

// ---------------------------------------------------------------------------
// Nested change enums — typed field mutations
// ---------------------------------------------------------------------------

/// A typed change to a Source entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum SourceChange {
    Weight { old: f64, new: f64 },
    Url { old: String, new: String },
    Role { old: SourceRole, new: SourceRole },
    QualityPenalty { old: f64, new: f64 },
    GapContext { old: Option<String>, new: Option<String> },
    Active { old: bool, new: bool },
}

/// A typed change to a Situation entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum SituationChange {
    Headline { old: String, new: String },
    Lede { old: String, new: String },
    Arc { old: SituationArc, new: SituationArc },
    Temperature { old: f64, new: f64 },
    Location { old: Option<Location>, new: Option<Location> },
    Sensitivity { old: SensitivityLevel, new: SensitivityLevel },
    Category { old: Option<String>, new: Option<String> },
    StructuredState { old: String, new: String },
}

// ---------------------------------------------------------------------------
// Per-entity correction enums — each only has fields that exist on that type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum GatheringCorrection {
    Title { old: String, new: String },
    Summary { old: String, new: String },
    Confidence { old: f32, new: f32 },
    Sensitivity { old: SensitivityLevel, new: SensitivityLevel },
    Location { old: Option<Location>, new: Option<Location> },
    Schedule { old: Option<Schedule>, new: Option<Schedule> },
    Organizer { old: Option<String>, new: Option<String> },
    ActionUrl { old: Option<String>, new: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum AidCorrection {
    Title { old: String, new: String },
    Summary { old: String, new: String },
    Confidence { old: f32, new: f32 },
    Sensitivity { old: SensitivityLevel, new: SensitivityLevel },
    Location { old: Option<Location>, new: Option<Location> },
    ActionUrl { old: Option<String>, new: Option<String> },
    Availability { old: Option<String>, new: Option<String> },
    IsOngoing { old: Option<bool>, new: Option<bool> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum NeedCorrection {
    Title { old: String, new: String },
    Summary { old: String, new: String },
    Confidence { old: f32, new: f32 },
    Sensitivity { old: SensitivityLevel, new: SensitivityLevel },
    Location { old: Option<Location>, new: Option<Location> },
    Urgency { old: Option<Urgency>, new: Option<Urgency> },
    WhatNeeded { old: Option<String>, new: Option<String> },
    Goal { old: Option<String>, new: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum NoticeCorrection {
    Title { old: String, new: String },
    Summary { old: String, new: String },
    Confidence { old: f32, new: f32 },
    Sensitivity { old: SensitivityLevel, new: SensitivityLevel },
    Location { old: Option<Location>, new: Option<Location> },
    Severity { old: Option<Severity>, new: Option<Severity> },
    Category { old: Option<String>, new: Option<String> },
    EffectiveDate { old: Option<DateTime<Utc>>, new: Option<DateTime<Utc>> },
    SourceAuthority { old: Option<String>, new: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum TensionCorrection {
    Title { old: String, new: String },
    Summary { old: String, new: String },
    Confidence { old: f32, new: f32 },
    Sensitivity { old: SensitivityLevel, new: SensitivityLevel },
    Location { old: Option<Location>, new: Option<Location> },
    Severity { old: Option<Severity>, new: Option<Severity> },
    WhatWouldHelp { old: Option<String>, new: Option<String> },
}

// ---------------------------------------------------------------------------
// The Event enum — facts about what happened
// ---------------------------------------------------------------------------

/// A fact about what happened. Infrastructure-agnostic, human-readable.
///
/// The `type` tag becomes the `event_type` column in the events table.
/// The rest serializes to the `payload` JSONB column.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    // -----------------------------------------------------------------------
    // Observability facts (migrated from scout_run_events)
    // Graph reducer: no-op on all of these.
    // -----------------------------------------------------------------------
    UrlScraped {
        url: String,
        strategy: String,
        success: bool,
        content_bytes: usize,
    },

    FeedScraped {
        url: String,
        items: u32,
    },

    SocialScraped {
        platform: String,
        identifier: String,
        post_count: u32,
    },

    SocialTopicsSearched {
        platform: String,
        topics: Vec<String>,
        posts_found: u32,
    },

    SearchPerformed {
        query: String,
        provider: String,
        result_count: u32,
        canonical_key: String,
    },

    LlmExtractionCompleted {
        source_url: String,
        content_chars: usize,
        entities_extracted: u32,
        implied_queries: u32,
    },

    BudgetCheckpoint {
        spent_cents: u64,
        remaining_cents: u64,
    },

    BootstrapCompleted {
        sources_created: u64,
    },

    AgentWebSearched {
        provider: String,
        query: String,
        result_count: u32,
        title: String,
    },

    AgentPageRead {
        provider: String,
        url: String,
        content_chars: usize,
        title: String,
    },

    AgentFutureQuery {
        provider: String,
        query: String,
        title: String,
    },

    // -----------------------------------------------------------------------
    // Discovery facts — 5 typed variants, each owns all its fields
    // -----------------------------------------------------------------------
    GatheringDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        sensitivity: SensitivityLevel,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        content_date: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        implied_queries: Vec<String>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        // Gathering-specific
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        organizer: Option<String>,
    },

    AidDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        sensitivity: SensitivityLevel,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        content_date: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        implied_queries: Vec<String>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        // Aid-specific
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        availability: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_ongoing: Option<bool>,
    },

    NeedDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        sensitivity: SensitivityLevel,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        content_date: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        implied_queries: Vec<String>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        // Need-specific
        #[serde(default, skip_serializing_if = "Option::is_none")]
        urgency: Option<Urgency>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        what_needed: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal: Option<String>,
    },

    NoticeDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        sensitivity: SensitivityLevel,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        content_date: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        implied_queries: Vec<String>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        // Notice-specific
        #[serde(default, skip_serializing_if = "Option::is_none")]
        severity: Option<Severity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_date: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_authority: Option<String>,
    },

    TensionDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        sensitivity: SensitivityLevel,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        content_date: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        implied_queries: Vec<String>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        // Tension-specific
        #[serde(default, skip_serializing_if = "Option::is_none")]
        severity: Option<Severity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        what_would_help: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Observation lifecycle facts
    // -----------------------------------------------------------------------
    ObservationCorroborated {
        entity_id: Uuid,
        node_type: NodeType,
        new_source_url: String,
        similarity: f64,
        new_corroboration_count: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    FreshnessConfirmed {
        entity_ids: Vec<Uuid>,
        node_type: NodeType,
        confirmed_at: DateTime<Utc>,
    },

    ConfidenceScored {
        entity_id: Uuid,
        old_confidence: f32,
        new_confidence: f32,
    },

    ObservationRejected {
        entity_id: Option<Uuid>,
        title: String,
        source_url: String,
        reason: String,
    },

    EntityExpired {
        entity_id: Uuid,
        node_type: NodeType,
        reason: String,
    },

    EntityPurged {
        entity_id: Uuid,
        node_type: NodeType,
        reason: String,
        context: Option<String>,
    },

    DuplicateDetected {
        node_type: NodeType,
        title: String,
        matched_id: Uuid,
        similarity: f64,
        action: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    ExtractionDroppedNoDate {
        title: String,
        source_url: String,
    },

    ReviewVerdictReached {
        entity_id: Uuid,
        old_status: String,
        new_status: String,
        reason: String,
    },

    ImpliedQueriesConsumed {
        entity_ids: Vec<Uuid>,
    },

    // -----------------------------------------------------------------------
    // Correction facts — one event per entity type, typed inner enum
    // -----------------------------------------------------------------------
    GatheringCorrected {
        entity_id: Uuid,
        correction: GatheringCorrection,
        reason: String,
    },

    AidCorrected {
        entity_id: Uuid,
        correction: AidCorrection,
        reason: String,
    },

    NeedCorrected {
        entity_id: Uuid,
        correction: NeedCorrection,
        reason: String,
    },

    NoticeCorrected {
        entity_id: Uuid,
        correction: NoticeCorrection,
        reason: String,
    },

    TensionCorrected {
        entity_id: Uuid,
        correction: TensionCorrection,
        reason: String,
    },

    // -----------------------------------------------------------------------
    // Citation facts
    // -----------------------------------------------------------------------
    CitationRecorded {
        citation_id: Uuid,
        entity_id: Uuid,
        url: String,
        content_hash: String,
        snippet: Option<String>,
        relevance: Option<String>,
        channel_type: Option<ChannelType>,
        evidence_confidence: Option<f32>,
    },

    OrphanedCitationsCleaned {
        citation_ids: Vec<Uuid>,
    },

    // -----------------------------------------------------------------------
    // Source facts
    // -----------------------------------------------------------------------
    SourceRegistered {
        source_id: Uuid,
        canonical_key: String,
        canonical_value: String,
        url: Option<String>,
        discovery_method: DiscoveryMethod,
        weight: f64,
        source_role: SourceRole,
        gap_context: Option<String>,
    },

    SourceChanged {
        source_id: Uuid,
        canonical_key: String,
        change: SourceChange,
    },

    SourceDeactivated {
        source_ids: Vec<Uuid>,
        reason: String,
    },

    SourceRemoved {
        source_id: Uuid,
        canonical_key: String,
    },

    SourceScrapeRecorded {
        canonical_key: String,
        entities_produced: u32,
        scrape_count: u32,
        consecutive_empty_runs: u32,
    },

    SourceLinkDiscovered {
        child_id: Uuid,
        parent_canonical_key: String,
    },

    ExpansionQueryCollected {
        query: String,
        source_url: String,
    },

    ExpansionSourceCreated {
        canonical_key: String,
        query: String,
        source_url: String,
    },

    // -----------------------------------------------------------------------
    // Actor facts
    // -----------------------------------------------------------------------
    ActorIdentified {
        actor_id: Uuid,
        name: String,
        actor_type: ActorType,
        entity_id: String,
        domains: Vec<String>,
        social_urls: Vec<String>,
        description: String,
    },

    ActorLinkedToEntity {
        actor_id: Uuid,
        entity_id: Uuid,
        role: String,
    },

    ActorLinkedToSource {
        actor_id: Uuid,
        source_id: Uuid,
    },

    ActorStatsUpdated {
        actor_id: Uuid,
        entity_count: u32,
        last_active: DateTime<Utc>,
    },

    ActorLocationIdentified {
        actor_id: Uuid,
        location_lat: f64,
        location_lng: f64,
        location_name: Option<String>,
    },

    DuplicateActorsMerged {
        kept_id: Uuid,
        merged_ids: Vec<Uuid>,
    },

    OrphanedActorsCleaned {
        actor_ids: Vec<Uuid>,
    },

    // -----------------------------------------------------------------------
    // Situation / dispatch facts
    // -----------------------------------------------------------------------
    SituationIdentified {
        situation_id: Uuid,
        headline: String,
        lede: String,
        arc: SituationArc,
        temperature: f64,
        centroid_lat: Option<f64>,
        centroid_lng: Option<f64>,
        location_name: Option<String>,
        sensitivity: SensitivityLevel,
        category: Option<String>,
        structured_state: String,
    },

    SituationChanged {
        situation_id: Uuid,
        change: SituationChange,
    },

    SituationPromoted {
        situation_ids: Vec<Uuid>,
    },

    DispatchCreated {
        dispatch_id: Uuid,
        situation_id: Uuid,
        body: String,
        entity_ids: Vec<Uuid>,
        dispatch_type: DispatchType,
        supersedes: Option<Uuid>,
        fidelity_score: Option<f64>,
    },

    // -----------------------------------------------------------------------
    // Tag facts
    // -----------------------------------------------------------------------
    TagsAggregated {
        situation_id: Uuid,
        tags: Vec<TagFact>,
    },

    TagSuppressed {
        situation_id: Uuid,
        tag_slug: String,
    },

    TagsMerged {
        source_slug: String,
        target_slug: String,
    },

    // -----------------------------------------------------------------------
    // Quality / lint facts
    // -----------------------------------------------------------------------
    LintBatchCompleted {
        source_url: String,
        entity_count: u32,
        passed: u32,
        corrected: u32,
        rejected: u32,
    },

    LintCorrectionApplied {
        node_id: Uuid,
        node_type: NodeType,
        title: String,
        field: String,
        old_value: String,
        new_value: String,
        reason: String,
    },

    LintRejectionIssued {
        node_id: Uuid,
        node_type: NodeType,
        title: String,
        reason: String,
    },

    EmptyEntitiesCleaned {
        entity_ids: Vec<Uuid>,
    },

    FakeCoordinatesNulled {
        entity_ids: Vec<Uuid>,
        old_coords: Vec<(f64, f64)>,
    },

    // -----------------------------------------------------------------------
    // Pin / demand / submission facts
    // -----------------------------------------------------------------------
    PinCreated {
        pin_id: Uuid,
        location_lat: f64,
        location_lng: f64,
        source_id: Uuid,
        created_by: String,
    },

    PinsRemoved {
        pin_ids: Vec<Uuid>,
    },

    DemandReceived {
        demand_id: Uuid,
        query: String,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
    },

    DemandAggregated {
        created_task_ids: Vec<Uuid>,
        consumed_demand_ids: Vec<Uuid>,
    },

    SubmissionReceived {
        submission_id: Uuid,
        url: String,
        reason: Option<String>,
        source_canonical_key: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Event → event_type string (for the events table column)
// ---------------------------------------------------------------------------

impl Event {
    /// The snake_case event type string for this variant.
    pub fn event_type(&self) -> &'static str {
        match self {
            // Observability
            Event::UrlScraped { .. } => "url_scraped",
            Event::FeedScraped { .. } => "feed_scraped",
            Event::SocialScraped { .. } => "social_scraped",
            Event::SocialTopicsSearched { .. } => "social_topics_searched",
            Event::SearchPerformed { .. } => "search_performed",
            Event::LlmExtractionCompleted { .. } => "llm_extraction_completed",
            Event::BudgetCheckpoint { .. } => "budget_checkpoint",
            Event::BootstrapCompleted { .. } => "bootstrap_completed",
            Event::AgentWebSearched { .. } => "agent_web_searched",
            Event::AgentPageRead { .. } => "agent_page_read",
            Event::AgentFutureQuery { .. } => "agent_future_query",
            // Discovery
            Event::GatheringDiscovered { .. } => "gathering_discovered",
            Event::AidDiscovered { .. } => "aid_discovered",
            Event::NeedDiscovered { .. } => "need_discovered",
            Event::NoticeDiscovered { .. } => "notice_discovered",
            Event::TensionDiscovered { .. } => "tension_discovered",
            // Observation lifecycle
            Event::ObservationCorroborated { .. } => "observation_corroborated",
            Event::FreshnessConfirmed { .. } => "freshness_confirmed",
            Event::ConfidenceScored { .. } => "confidence_scored",
            Event::ObservationRejected { .. } => "observation_rejected",
            Event::EntityExpired { .. } => "entity_expired",
            Event::EntityPurged { .. } => "entity_purged",
            Event::DuplicateDetected { .. } => "duplicate_detected",
            Event::ExtractionDroppedNoDate { .. } => "extraction_dropped_no_date",
            Event::ReviewVerdictReached { .. } => "review_verdict_reached",
            Event::ImpliedQueriesConsumed { .. } => "implied_queries_consumed",
            // Corrections
            Event::GatheringCorrected { .. } => "gathering_corrected",
            Event::AidCorrected { .. } => "aid_corrected",
            Event::NeedCorrected { .. } => "need_corrected",
            Event::NoticeCorrected { .. } => "notice_corrected",
            Event::TensionCorrected { .. } => "tension_corrected",
            // Citations
            Event::CitationRecorded { .. } => "citation_recorded",
            Event::OrphanedCitationsCleaned { .. } => "orphaned_citations_cleaned",
            // Sources
            Event::SourceRegistered { .. } => "source_registered",
            Event::SourceChanged { .. } => "source_changed",
            Event::SourceDeactivated { .. } => "source_deactivated",
            Event::SourceRemoved { .. } => "source_removed",
            Event::SourceScrapeRecorded { .. } => "source_scrape_recorded",
            Event::SourceLinkDiscovered { .. } => "source_link_discovered",
            Event::ExpansionQueryCollected { .. } => "expansion_query_collected",
            Event::ExpansionSourceCreated { .. } => "expansion_source_created",
            // Actors
            Event::ActorIdentified { .. } => "actor_identified",
            Event::ActorLinkedToEntity { .. } => "actor_linked_to_entity",
            Event::ActorLinkedToSource { .. } => "actor_linked_to_source",
            Event::ActorStatsUpdated { .. } => "actor_stats_updated",
            Event::ActorLocationIdentified { .. } => "actor_location_identified",
            Event::DuplicateActorsMerged { .. } => "duplicate_actors_merged",
            Event::OrphanedActorsCleaned { .. } => "orphaned_actors_cleaned",
            // Situations / dispatches
            Event::SituationIdentified { .. } => "situation_identified",
            Event::SituationChanged { .. } => "situation_changed",
            Event::SituationPromoted { .. } => "situation_promoted",
            Event::DispatchCreated { .. } => "dispatch_created",
            // Tags
            Event::TagsAggregated { .. } => "tags_aggregated",
            Event::TagSuppressed { .. } => "tag_suppressed",
            Event::TagsMerged { .. } => "tags_merged",
            // Quality / lint
            Event::LintBatchCompleted { .. } => "lint_batch_completed",
            Event::LintCorrectionApplied { .. } => "lint_correction_applied",
            Event::LintRejectionIssued { .. } => "lint_rejection_issued",
            Event::EmptyEntitiesCleaned { .. } => "empty_entities_cleaned",
            Event::FakeCoordinatesNulled { .. } => "fake_coordinates_nulled",
            // Pins / demand / submissions
            Event::PinCreated { .. } => "pin_created",
            Event::PinsRemoved { .. } => "pins_removed",
            Event::DemandReceived { .. } => "demand_received",
            Event::DemandAggregated { .. } => "demand_aggregated",
            Event::SubmissionReceived { .. } => "submission_received",
        }
    }

    /// Serialize this event to a JSON Value for the EventStore payload.
    pub fn to_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("Event serialization should never fail")
    }

    /// Deserialize an event from a JSON payload + event_type.
    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(payload.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_type_matches_serde_tag() {
        let event = Event::UrlScraped {
            url: "https://example.com".into(),
            strategy: "web_page".into(),
            success: true,
            content_bytes: 1024,
        };
        assert_eq!(event.event_type(), "url_scraped");

        let json = event.to_payload();
        assert_eq!(json["type"].as_str().unwrap(), "url_scraped");
    }

    #[test]
    fn gathering_discovered_roundtrip() {
        let event = Event::GatheringDiscovered {
            id: Uuid::new_v4(),
            title: "Community Cleanup".into(),
            summary: "Monthly neighborhood cleanup".into(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.85,
            source_url: "https://example.com/cleanup".into(),
            extracted_at: Utc::now(),
            content_date: Some(Utc::now()),
            location: Some(Location {
                point: Some(GeoPoint {
                    lat: 44.9778,
                    lng: -93.265,
                    precision: crate::types::GeoPrecision::Neighborhood,
                }),
                name: Some("Minneapolis".into()),
                address: None,
            }),
            from_location: None,
            implied_queries: vec!["cleanup Minneapolis".into()],
            mentioned_actors: vec!["Lake Street Council".into()],
            author_actor: None,
            schedule: Some(Schedule {
                starts_at: Some(Utc::now()),
                ends_at: None,
                all_day: false,
                rrule: Some("FREQ=MONTHLY;BYDAY=1SA".into()),
                timezone: Some("America/Chicago".into()),
            }),
            action_url: Some("https://example.com/signup".into()),
            organizer: Some("Lake Street Council".into()),
        };

        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();

        match roundtripped {
            Event::GatheringDiscovered { title, confidence, schedule, .. } => {
                assert_eq!(title, "Community Cleanup");
                assert!((confidence - 0.85).abs() < f32::EPSILON);
                assert!(schedule.is_some());
                assert_eq!(schedule.unwrap().rrule.unwrap(), "FREQ=MONTHLY;BYDAY=1SA");
            }
            _ => panic!("Expected GatheringDiscovered"),
        }
    }

    #[test]
    fn aid_discovered_roundtrip() {
        let event = Event::AidDiscovered {
            id: Uuid::new_v4(),
            title: "Food Shelf".into(),
            summary: "Free groceries".into(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.9,
            source_url: "https://example.com/food".into(),
            extracted_at: Utc::now(),
            content_date: None,
            location: None,
            from_location: None,
            implied_queries: vec![],
            mentioned_actors: vec![],
            author_actor: None,
            action_url: Some("https://example.com/food".into()),
            availability: Some("Monday-Friday 9am-5pm".into()),
            is_ongoing: Some(true),
        };

        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();

        match roundtripped {
            Event::AidDiscovered { title, availability, .. } => {
                assert_eq!(title, "Food Shelf");
                assert_eq!(availability.unwrap(), "Monday-Friday 9am-5pm");
            }
            _ => panic!("Expected AidDiscovered"),
        }
    }

    #[test]
    fn tension_discovered_roundtrip() {
        let event = Event::TensionDiscovered {
            id: Uuid::new_v4(),
            title: "Housing Shortage".into(),
            summary: "Affordable units declining".into(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.75,
            source_url: "https://example.com/housing".into(),
            extracted_at: Utc::now(),
            content_date: None,
            location: None,
            from_location: None,
            implied_queries: vec![],
            mentioned_actors: vec![],
            author_actor: None,
            severity: Some(Severity::High),
            what_would_help: Some("More affordable housing construction".into()),
        };

        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();

        match roundtripped {
            Event::TensionDiscovered { title, severity, .. } => {
                assert_eq!(title, "Housing Shortage");
                assert_eq!(severity.unwrap(), Severity::High);
            }
            _ => panic!("Expected TensionDiscovered"),
        }
    }

    #[test]
    fn source_change_nested_enum_roundtrip() {
        let event = Event::SourceChanged {
            source_id: Uuid::new_v4(),
            canonical_key: "web:example.com".into(),
            change: SourceChange::Weight { old: 0.5, new: 0.8 },
        };

        let payload = event.to_payload();
        let json_change = &payload["change"];
        assert_eq!(json_change["field"].as_str().unwrap(), "weight");

        let roundtripped = Event::from_payload(&payload).unwrap();
        match roundtripped {
            Event::SourceChanged { change: SourceChange::Weight { old, new }, .. } => {
                assert!((old - 0.5).abs() < f64::EPSILON);
                assert!((new - 0.8).abs() < f64::EPSILON);
            }
            _ => panic!("Expected SourceChanged::Weight"),
        }
    }

    #[test]
    fn situation_change_nested_enum_roundtrip() {
        let event = Event::SituationChanged {
            situation_id: Uuid::new_v4(),
            change: SituationChange::Arc {
                old: SituationArc::Emerging,
                new: SituationArc::Developing,
            },
        };

        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();
        match roundtripped {
            Event::SituationChanged { change: SituationChange::Arc { old, new }, .. } => {
                assert_eq!(old, SituationArc::Emerging);
                assert_eq!(new, SituationArc::Developing);
            }
            _ => panic!("Expected SituationChanged::Arc"),
        }
    }

    #[test]
    fn gathering_correction_roundtrip() {
        let event = Event::GatheringCorrected {
            entity_id: Uuid::new_v4(),
            correction: GatheringCorrection::Title {
                old: "Commuinty Cleanup".into(),
                new: "Community Cleanup".into(),
            },
            reason: "Typo in title".into(),
        };

        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();
        match roundtripped {
            Event::GatheringCorrected { correction: GatheringCorrection::Title { old, new }, reason, .. } => {
                assert_eq!(old, "Commuinty Cleanup");
                assert_eq!(new, "Community Cleanup");
                assert_eq!(reason, "Typo in title");
            }
            _ => panic!("Expected GatheringCorrected::Title"),
        }
    }

    #[test]
    fn all_event_types_are_unique() {
        let types = vec![
            Event::UrlScraped { url: String::new(), strategy: String::new(), success: true, content_bytes: 0 }.event_type(),
            Event::GatheringDiscovered {
                id: Uuid::nil(), title: String::new(), summary: String::new(),
                sensitivity: SensitivityLevel::General, confidence: 0.0,
                source_url: String::new(), extracted_at: Utc::now(),
                content_date: None, location: None, from_location: None,
                implied_queries: vec![], mentioned_actors: vec![],
                author_actor: None, schedule: None, action_url: None, organizer: None,
            }.event_type(),
            Event::AidDiscovered {
                id: Uuid::nil(), title: String::new(), summary: String::new(),
                sensitivity: SensitivityLevel::General, confidence: 0.0,
                source_url: String::new(), extracted_at: Utc::now(),
                content_date: None, location: None, from_location: None,
                implied_queries: vec![], mentioned_actors: vec![],
                author_actor: None, action_url: None, availability: None, is_ongoing: None,
            }.event_type(),
            Event::TensionDiscovered {
                id: Uuid::nil(), title: String::new(), summary: String::new(),
                sensitivity: SensitivityLevel::General, confidence: 0.0,
                source_url: String::new(), extracted_at: Utc::now(),
                content_date: None, location: None, from_location: None,
                implied_queries: vec![], mentioned_actors: vec![],
                author_actor: None, severity: None, what_would_help: None,
            }.event_type(),
            Event::CitationRecorded {
                citation_id: Uuid::nil(), entity_id: Uuid::nil(),
                url: String::new(), content_hash: String::new(),
                snippet: None, relevance: None, channel_type: None, evidence_confidence: None,
            }.event_type(),
            Event::SourceChanged {
                source_id: Uuid::nil(), canonical_key: String::new(),
                change: SourceChange::Weight { old: 0.0, new: 0.0 },
            }.event_type(),
            Event::GatheringCorrected {
                entity_id: Uuid::nil(),
                correction: GatheringCorrection::Title { old: String::new(), new: String::new() },
                reason: String::new(),
            }.event_type(),
        ];
        let unique: std::collections::HashSet<&str> = types.iter().copied().collect();
        assert_eq!(types.len(), unique.len(), "Duplicate event_type strings found");
    }

    #[test]
    fn schedule_optional_fields_deserialize_from_minimal_json() {
        let json = serde_json::json!({
            "starts_at": "2026-03-08T19:00:00Z"
        });
        let schedule: Schedule = serde_json::from_value(json).unwrap();
        assert!(schedule.starts_at.is_some());
        assert!(schedule.ends_at.is_none());
        assert!(!schedule.all_day);
        assert!(schedule.rrule.is_none());
        assert!(schedule.timezone.is_none());
    }

    #[test]
    fn location_optional_fields_deserialize_from_minimal_json() {
        let json = serde_json::json!({
            "name": "Lake Harriet"
        });
        let loc: Location = serde_json::from_value(json).unwrap();
        assert!(loc.point.is_none());
        assert_eq!(loc.name.unwrap(), "Lake Harriet");
        assert!(loc.address.is_none());
    }
}
