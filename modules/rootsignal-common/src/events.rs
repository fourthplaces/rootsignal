//! Unified event enum — facts about what happened in the world.
//!
//! Every variant describes something the system observed or decided.
//! No embeddings, no derived metrics, no infrastructure artifacts.
//! Events serialize to `serde_json::Value` for the generic EventStore.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::safety::SensitivityLevel;
use crate::types::{
    ActorType, ChannelType, DiscoveryMethod, DispatchType, GeoPoint, NodeType, Severity,
    SituationArc, SourceRole, Urgency,
};

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
        signals_extracted: u32,
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
    // Signal facts — reducer creates/updates/deletes graph nodes
    // -----------------------------------------------------------------------
    SignalDiscovered {
        signal_id: Uuid,
        node_type: NodeType,
        title: String,
        summary: String,
        sensitivity: SensitivityLevel,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        content_date: Option<DateTime<Utc>>,
        about_location: Option<GeoPoint>,
        about_location_name: Option<String>,
        from_location: Option<GeoPoint>,
        implied_queries: Vec<String>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        // Type-specific fields (all optional — set based on node_type)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        starts_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        organizer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_recurring: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        availability: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_ongoing: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        urgency: Option<Urgency>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        what_needed: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        severity: Option<Severity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_date: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_authority: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        what_would_help: Option<String>,
    },

    SignalCorroborated {
        signal_id: Uuid,
        node_type: NodeType,
        new_source_url: String,
        similarity: f64,
        new_corroboration_count: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    SignalRefreshed {
        signal_ids: Vec<Uuid>,
        node_type: NodeType,
        new_last_confirmed_active: DateTime<Utc>,
    },

    SignalConfidenceScored {
        signal_id: Uuid,
        old_confidence: f32,
        new_confidence: f32,
    },

    SignalFieldsCorrected {
        signal_id: Uuid,
        corrections: Vec<FieldCorrection>,
    },

    SignalRejected {
        signal_id: Option<Uuid>,
        title: String,
        source_url: String,
        reason: String,
    },

    SignalExpired {
        signal_id: Uuid,
        node_type: NodeType,
        reason: String,
    },

    SignalPurged {
        signal_id: Uuid,
        node_type: NodeType,
        reason: String,
        context: Option<String>,
    },

    SignalDeduplicated {
        signal_type: NodeType,
        title: String,
        matched_id: Uuid,
        similarity: f64,
        action: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    SignalDroppedNoDate {
        title: String,
        source_url: String,
    },

    ReviewVerdictReached {
        signal_id: Uuid,
        old_status: String,
        new_status: String,
        reason: String,
    },

    ImpliedQueriesConsumed {
        signal_ids: Vec<Uuid>,
    },

    // -----------------------------------------------------------------------
    // Citation facts
    // -----------------------------------------------------------------------
    CitationRecorded {
        citation_id: Uuid,
        signal_id: Uuid,
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

    SourceUpdated {
        source_id: Uuid,
        canonical_key: String,
        changes: serde_json::Value,
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
        signals_produced: u32,
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

    ActorLinkedToSignal {
        actor_id: Uuid,
        signal_id: Uuid,
        role: String,
    },

    ActorLinkedToSource {
        actor_id: Uuid,
        source_id: Uuid,
    },

    ActorStatsUpdated {
        actor_id: Uuid,
        signal_count: u32,
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
    // Relationship facts
    // -----------------------------------------------------------------------
    RelationshipEstablished {
        from_id: Uuid,
        to_id: Uuid,
        relationship_type: String,
        properties: Option<serde_json::Value>,
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

    SituationEvolved {
        situation_id: Uuid,
        changes: serde_json::Value,
    },

    SituationPromoted {
        situation_ids: Vec<Uuid>,
    },

    DispatchCreated {
        dispatch_id: Uuid,
        situation_id: Uuid,
        body: String,
        signal_ids: Vec<Uuid>,
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
        signal_count: u32,
        passed: u32,
        corrected: u32,
        rejected: u32,
    },

    LintCorrectionApplied {
        node_id: Uuid,
        signal_type: NodeType,
        title: String,
        field: String,
        old_value: String,
        new_value: String,
        reason: String,
    },

    LintRejectionIssued {
        node_id: Uuid,
        signal_type: NodeType,
        title: String,
        reason: String,
    },

    EmptySignalsCleaned {
        signal_ids: Vec<Uuid>,
    },

    FakeCoordinatesNulled {
        signal_ids: Vec<Uuid>,
        old_coords: Vec<(f64, f64)>,
    },

    // -----------------------------------------------------------------------
    // Schedule facts
    // -----------------------------------------------------------------------
    ScheduleRecorded {
        signal_id: Uuid,
        rrule: String,
        dtstart: DateTime<Utc>,
        label: Option<String>,
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

    DemandSignalReceived {
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
// Supporting types for event payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldCorrection {
    pub field: String,
    pub old_value: serde_json::Value,
    pub new_value: serde_json::Value,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagFact {
    pub slug: String,
    pub name: String,
    pub weight: f64,
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
            // Signals
            Event::SignalDiscovered { .. } => "signal_discovered",
            Event::SignalCorroborated { .. } => "signal_corroborated",
            Event::SignalRefreshed { .. } => "signal_refreshed",
            Event::SignalConfidenceScored { .. } => "signal_confidence_scored",
            Event::SignalFieldsCorrected { .. } => "signal_fields_corrected",
            Event::SignalRejected { .. } => "signal_rejected",
            Event::SignalExpired { .. } => "signal_expired",
            Event::SignalPurged { .. } => "signal_purged",
            Event::SignalDeduplicated { .. } => "signal_deduplicated",
            Event::SignalDroppedNoDate { .. } => "signal_dropped_no_date",
            Event::ReviewVerdictReached { .. } => "review_verdict_reached",
            Event::ImpliedQueriesConsumed { .. } => "implied_queries_consumed",
            // Citations
            Event::CitationRecorded { .. } => "citation_recorded",
            Event::OrphanedCitationsCleaned { .. } => "orphaned_citations_cleaned",
            // Sources
            Event::SourceRegistered { .. } => "source_registered",
            Event::SourceUpdated { .. } => "source_updated",
            Event::SourceDeactivated { .. } => "source_deactivated",
            Event::SourceRemoved { .. } => "source_removed",
            Event::SourceScrapeRecorded { .. } => "source_scrape_recorded",
            Event::SourceLinkDiscovered { .. } => "source_link_discovered",
            Event::ExpansionQueryCollected { .. } => "expansion_query_collected",
            Event::ExpansionSourceCreated { .. } => "expansion_source_created",
            // Actors
            Event::ActorIdentified { .. } => "actor_identified",
            Event::ActorLinkedToSignal { .. } => "actor_linked_to_signal",
            Event::ActorLinkedToSource { .. } => "actor_linked_to_source",
            Event::ActorStatsUpdated { .. } => "actor_stats_updated",
            Event::ActorLocationIdentified { .. } => "actor_location_identified",
            Event::DuplicateActorsMerged { .. } => "duplicate_actors_merged",
            Event::OrphanedActorsCleaned { .. } => "orphaned_actors_cleaned",
            // Relationships
            Event::RelationshipEstablished { .. } => "relationship_established",
            // Situations / dispatches
            Event::SituationIdentified { .. } => "situation_identified",
            Event::SituationEvolved { .. } => "situation_evolved",
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
            Event::EmptySignalsCleaned { .. } => "empty_signals_cleaned",
            Event::FakeCoordinatesNulled { .. } => "fake_coordinates_nulled",
            // Schedule
            Event::ScheduleRecorded { .. } => "schedule_recorded",
            // Pins / demand / submissions
            Event::PinCreated { .. } => "pin_created",
            Event::PinsRemoved { .. } => "pins_removed",
            Event::DemandSignalReceived { .. } => "demand_signal_received",
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

        // Verify the serde tag matches
        let json = event.to_payload();
        assert_eq!(json["type"].as_str().unwrap(), "url_scraped");
    }

    #[test]
    fn signal_discovered_roundtrip() {
        let event = Event::SignalDiscovered {
            signal_id: Uuid::new_v4(),
            node_type: NodeType::Gathering,
            title: "Community Cleanup".into(),
            summary: "Monthly neighborhood cleanup".into(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.85,
            source_url: "https://example.com/cleanup".into(),
            extracted_at: Utc::now(),
            content_date: Some(Utc::now()),
            about_location: Some(GeoPoint {
                lat: 44.9778,
                lng: -93.265,
                precision: crate::types::GeoPrecision::Neighborhood,
            }),
            about_location_name: Some("Minneapolis".into()),
            from_location: None,
            implied_queries: vec!["cleanup Minneapolis".into()],
            mentioned_actors: vec!["Lake Street Council".into()],
            author_actor: None,
            starts_at: Some(Utc::now()),
            ends_at: None,
            action_url: Some("https://example.com/signup".into()),
            organizer: Some("Lake Street Council".into()),
            is_recurring: Some(true),
            availability: None,
            is_ongoing: None,
            urgency: None,
            what_needed: None,
            goal: None,
            severity: None,
            category: None,
            effective_date: None,
            source_authority: None,
            what_would_help: None,
        };

        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();

        match roundtripped {
            Event::SignalDiscovered { title, node_type, confidence, .. } => {
                assert_eq!(title, "Community Cleanup");
                assert_eq!(node_type, NodeType::Gathering);
                assert!((confidence - 0.85).abs() < f32::EPSILON);
            }
            _ => panic!("Expected SignalDiscovered"),
        }
    }

    #[test]
    fn all_event_types_are_unique() {
        // Compile-time guarantee: each variant has a unique event_type string.
        // This test verifies a representative sample.
        let types = vec![
            Event::UrlScraped { url: String::new(), strategy: String::new(), success: true, content_bytes: 0 }.event_type(),
            Event::SignalDiscovered {
                signal_id: Uuid::nil(), node_type: NodeType::Gathering,
                title: String::new(), summary: String::new(),
                sensitivity: SensitivityLevel::General, confidence: 0.0,
                source_url: String::new(), extracted_at: Utc::now(),
                content_date: None, about_location: None, about_location_name: None,
                from_location: None, implied_queries: vec![], mentioned_actors: vec![],
                author_actor: None, starts_at: None, ends_at: None, action_url: None,
                organizer: None, is_recurring: None, availability: None, is_ongoing: None,
                urgency: None, what_needed: None, goal: None, severity: None, category: None,
                effective_date: None, source_authority: None, what_would_help: None,
            }.event_type(),
            Event::CitationRecorded {
                citation_id: Uuid::nil(), signal_id: Uuid::nil(),
                url: String::new(), content_hash: String::new(),
                snippet: None, relevance: None, channel_type: None, evidence_confidence: None,
            }.event_type(),
        ];
        let unique: std::collections::HashSet<&str> = types.iter().copied().collect();
        assert_eq!(types.len(), unique.len(), "Duplicate event_type strings found");
    }
}
