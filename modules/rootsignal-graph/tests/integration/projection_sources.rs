use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::events::{Event, SystemEvent, WorldEvent};
use rootsignal_common::{DiscoveryMethod, SensitivityLevel, SituationArc, SourceNode, SourceRole};
use rootsignal_graph::GraphProjector;

use super::helpers::*;

// ── Source Registration ────────────────────────────────────────────

#[tokio::test]
async fn sources_registered_creates_nodes() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let src_a = SourceNode::new(
        "https://patch.com".into(),
        "patch.com".into(),
        Some("https://patch.com".into()),
        DiscoveryMethod::Curated,
        0.8,
        SourceRole::Mixed,
        None,
    );
    let src_b = SourceNode::new(
        "https://mpr.org".into(),
        "mpr.org".into(),
        Some("https://mpr.org".into()),
        DiscoveryMethod::GapAnalysis,
        0.6,
        SourceRole::Concern,
        None,
    );
    let a_id = src_a.id;

    project_all(
        &projector,
        &[stored(
            1,
            &Event::System(SystemEvent::SourcesRegistered {
                sources: vec![src_a, src_b],
            }),
        )],
    )
    .await;

    assert_eq!(count_nodes(&client, "Source").await, 2);

    let key: String = read_prop(&client, "Source", a_id, "canonical_key").await;
    assert_eq!(key, "https://patch.com");

    let active: bool = read_prop(&client, "Source", a_id, "active").await;
    assert!(active);
}

#[tokio::test]
async fn source_deactivated_sets_inactive() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let source = SourceNode::new(
        "https://example.com".into(),
        "example.com".into(),
        None,
        DiscoveryMethod::Curated,
        0.5,
        SourceRole::Mixed,
        None,
    );
    let src_id = source.id;

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::SourcesRegistered {
                    sources: vec![source],
                }),
            ),
            stored(
                2,
                &Event::System(SystemEvent::SourceDeactivated {
                    source_ids: vec![src_id],
                    reason: "No signals".into(),
                }),
            ),
        ],
    )
    .await;

    let active: bool = read_prop(&client, "Source", src_id, "active").await;
    assert!(!active);
}

#[tokio::test]
async fn source_scraped_updates_counts() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let source = SourceNode::new(
        "https://patch.com/mn".into(),
        "patch.com/mn".into(),
        None,
        DiscoveryMethod::Curated,
        0.7,
        SourceRole::Mixed,
        None,
    );
    let src_id = source.id;

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::SourcesRegistered {
                    sources: vec![source],
                }),
            ),
            stored(
                2,
                &Event::System(SystemEvent::SourceScraped {
                    canonical_key: "https://patch.com/mn".into(),
                    signals_produced: 5,
                    scraped_at: Utc::now(),
                }),
            ),
        ],
    )
    .await;

    let produced: i64 = read_prop(&client, "Source", src_id, "signals_produced").await;
    assert_eq!(produced, 5);

    let scrape_count: i64 = read_prop(&client, "Source", src_id, "scrape_count").await;
    assert_eq!(scrape_count, 1);
}

#[tokio::test]
async fn source_scraped_replay_does_not_double_counts() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let source = SourceNode::new(
        "https://replay.com".into(),
        "replay.com".into(),
        None,
        DiscoveryMethod::Curated,
        0.5,
        SourceRole::Mixed,
        None,
    );
    let src_id = source.id;

    let scrape = Event::System(SystemEvent::SourceScraped {
        canonical_key: "https://replay.com".into(),
        signals_produced: 3,
        scraped_at: Utc::now(),
    });

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::SourcesRegistered {
                    sources: vec![source],
                }),
            ),
            stored(2, &scrape),
        ],
    )
    .await;

    // Replay the same scrape event
    project_all(&projector, &[stored(3, &scrape)]).await;

    let produced: i64 = read_prop(&client, "Source", src_id, "signals_produced").await;
    assert_eq!(produced, 3, "Replaying SourceScraped should not double signals_produced");

    let scrape_count: i64 = read_prop(&client, "Source", src_id, "scrape_count").await;
    assert_eq!(scrape_count, 1, "Replaying SourceScraped should not double scrape_count");
}

// ── Tagging ────────────────────────────────────────────────────────

#[tokio::test]
async fn signal_tagged_creates_tag_nodes_and_edges() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sig_id = Uuid::new_v4();
    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: sig_id,
                    title: "Community Garden".into(),
                    summary: "".into(),
                    url: "https://x.com/garden".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            stored(
                2,
                &Event::System(SystemEvent::SignalTagged {
                    signal_id: sig_id,
                    tag_slugs: vec!["community-garden".into(), "urban-farming".into()],
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_nodes(&client, "Tag").await, 2);
    assert_eq!(count_edges(&client, sig_id, "TAGGED", "Tag").await, 2);
}

#[tokio::test]
async fn same_tag_shared_by_two_signals_creates_one_node() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sig1 = Uuid::new_v4();
    let sig2 = Uuid::new_v4();

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: sig1,
                    title: "Potluck A".into(),
                    summary: "".into(),
                    url: "https://x.com/a".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            stored(
                2,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: sig2,
                    title: "Potluck B".into(),
                    summary: "".into(),
                    url: "https://x.com/b".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            stored(
                3,
                &Event::System(SystemEvent::SignalTagged {
                    signal_id: sig1,
                    tag_slugs: vec!["food-events".into()],
                }),
            ),
            stored(
                4,
                &Event::System(SystemEvent::SignalTagged {
                    signal_id: sig2,
                    tag_slugs: vec!["food-events".into()],
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_nodes(&client, "Tag").await, 1);
    assert_eq!(count_edges(&client, sig1, "TAGGED", "Tag").await, 1);
    assert_eq!(count_edges(&client, sig2, "TAGGED", "Tag").await, 1);
}

// ── Replay Idempotency ────────────────────────────────────────────

#[tokio::test]
async fn source_discovery_credit_replay_does_not_double_count() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let source = SourceNode::new(
        "https://discovery.com".into(),
        "discovery.com".into(),
        Some("https://discovery.com".into()),
        DiscoveryMethod::Curated,
        0.5,
        SourceRole::Mixed,
        None,
    );

    let credit = Event::System(SystemEvent::SourceDiscoveryCredit {
        canonical_key: "https://discovery.com".into(),
        sources_discovered: 3,
    });

    let credit_event = stored(2, &credit);

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::SourcesRegistered {
                    sources: vec![source],
                }),
            ),
            credit_event.clone(),
        ],
    )
    .await;

    // Replay the same PersistedEvent (same created_at, modeling true replay)
    project_all(&projector, &[credit_event]).await;

    let discovered: i64 = read_source_prop(&client, "https://discovery.com", "sources_discovered").await;
    assert_eq!(discovered, 3, "Replaying SourceDiscoveryCredit should not double sources_discovered");
}

#[tokio::test]
async fn source_boost_replay_does_not_compound_weight() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sig_id = Uuid::new_v4();
    let sit_id = Uuid::new_v4();
    let source = SourceNode::new(
        "https://boosted.com".into(),
        "boosted.com".into(),
        Some("https://boosted.com".into()),
        DiscoveryMethod::Curated,
        1.0,
        SourceRole::Mixed,
        None,
    );
    let src_id = source.id;

    let boost = Event::System(SystemEvent::SourcesBoostedForSituation {
        headline: "Housing crisis".into(),
        factor: 1.5,
    });

    let boost_event = stored(5, &boost);

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::SourcesRegistered {
                    sources: vec![source],
                }),
            ),
            stored(
                2,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: sig_id,
                    title: "Housing rally".into(),
                    summary: "".into(),
                    url: "https://boosted.com".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            stored(
                3,
                &Event::System(SystemEvent::SituationIdentified {
                    situation_id: sit_id,
                    headline: "Housing crisis".into(),
                    lede: "".into(),
                    arc: SituationArc::Emerging,
                    temperature: 0.5,
                    centroid_lat: None,
                    centroid_lng: None,
                    location_name: None,
                    sensitivity: SensitivityLevel::General,
                    category: None,
                    structured_state: "{}".into(),
                    tension_heat: None,
                    clarity: None,
                    signal_count: None,
                    narrative_embedding: None,
                    causal_embedding: None,
                    briefing_body: None,
                }),
            ),
            stored(
                4,
                &Event::System(SystemEvent::SignalAssignedToSituation {
                    signal_id: sig_id,
                    situation_id: sit_id,
                    signal_label: "Gathering".into(),
                    confidence: 0.9,
                    reasoning: "Related".into(),
                }),
            ),
            boost_event.clone(),
        ],
    )
    .await;

    let weight_after_first: f64 = read_prop(&client, "Source", src_id, "weight").await;
    assert!((weight_after_first - 1.5).abs() < 0.01, "First boost: 1.0 * 1.5 = 1.5");

    // Replay the same PersistedEvent (same created_at, modeling true replay)
    project_all(&projector, &[boost_event]).await;

    let weight_after_replay: f64 = read_prop(&client, "Source", src_id, "weight").await;
    assert!(
        (weight_after_replay - 1.5).abs() < 0.01,
        "Replaying SourcesBoostedForSituation should not compound weight. Expected 1.5, got {weight_after_replay}"
    );
}
