use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::events::{
    Event, GatheringCorrection, SituationChange, SystemEvent, WorldEvent,
};
use rootsignal_common::{DispatchType, NodeType, SensitivityLevel, SituationArc};
use rootsignal_graph::GraphProjector;

use super::helpers::*;

// ── Corrections ────────────────────────────────────────────────────

#[tokio::test]
async fn gathering_corrected_location_replaces_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let id = Uuid::new_v4();
    let old_loc = loc("Lake Harriet", 44.922, -93.309);
    let new_loc = loc("Lake Calhoun", 44.948, -93.311);

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id,
                    title: "Lakeside Concert".into(),
                    summary: "".into(),
                    url: "https://x.com/concert".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![old_loc.clone()],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            stored(
                2,
                &Event::System(SystemEvent::GatheringCorrected {
                    signal_id: id,
                    correction: GatheringCorrection::Location {
                        old: Some(old_loc),
                        new: Some(new_loc),
                    },
                    reason: "Wrong lake".into(),
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_edges(&client, id, "HELD_AT", "Location").await, 1);

    let name: String = {
        let cypher = "MATCH (:Gathering {id: $id})-[:HELD_AT]->(l:Location) RETURN l.name AS val";
        let q = rootsignal_graph::query(cypher).param("id", id.to_string());
        let mut stream = client.execute(q).await.expect("query");
        stream
            .next()
            .await
            .expect("stream")
            .map(|r| r.get::<String>("val").unwrap_or_default())
            .unwrap_or_default()
    };
    assert_eq!(name, "Lake Calhoun");
}

#[tokio::test]
async fn correction_to_none_removes_location_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let id = Uuid::new_v4();
    let old_loc = loc("Some Place", 44.95, -93.25);

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id,
                    title: "Event".into(),
                    summary: "".into(),
                    url: "https://x.com/ev".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![old_loc.clone()],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            stored(
                2,
                &Event::System(SystemEvent::GatheringCorrected {
                    signal_id: id,
                    correction: GatheringCorrection::Location {
                        old: Some(old_loc),
                        new: None,
                    },
                    reason: "Location was wrong".into(),
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_edges(&client, id, "HELD_AT", "Location").await, 0);
}

#[tokio::test]
async fn correction_removes_role_overridden_location_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let id = Uuid::new_v4();
    let old_loc = loc_with_role("Flood Zone", 44.934, -93.234, "affected_area");
    let new_loc = loc("City Hall", 44.977, -93.265);

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id,
                    title: "Relief Effort".into(),
                    summary: "".into(),
                    url: "https://x.com/relief".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![old_loc.clone()],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            stored(
                2,
                &Event::System(SystemEvent::GatheringCorrected {
                    signal_id: id,
                    correction: GatheringCorrection::Location {
                        old: Some(old_loc),
                        new: Some(new_loc),
                    },
                    reason: "Wrong location".into(),
                }),
            ),
        ],
    )
    .await;

    // The old AFFECTS edge (from role="affected_area") should be gone
    assert_eq!(count_edges(&client, id, "AFFECTS", "Location").await, 0,
        "Old role-overridden AFFECTS edge should be deleted by correction");
    // The new HELD_AT edge (default for Gathering) should exist
    assert_eq!(count_edges(&client, id, "HELD_AT", "Location").await, 1);
}

// ── FreshnessConfirmed ─────────────────────────────────────────────

#[tokio::test]
async fn freshness_confirmed_updates_batch() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let confirmed_at = Utc::now();

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: id1,
                    title: "Event A".into(),
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
                    id: id2,
                    title: "Event B".into(),
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
                &Event::System(SystemEvent::FreshnessConfirmed {
                    signal_ids: vec![id1, id2],
                    node_type: NodeType::Gathering,
                    confirmed_at,
                }),
            ),
        ],
    )
    .await;

    let ts1: String = read_prop(&client, "Gathering", id1, "last_confirmed_active").await;
    let ts2: String = read_prop(&client, "Gathering", id2, "last_confirmed_active").await;
    assert!(!ts1.is_empty(), "id1 should have last_confirmed_active set");
    assert!(!ts2.is_empty(), "id2 should have last_confirmed_active set");
}

// ── Situation Lifecycle ────────────────────────────────────────────

#[tokio::test]
async fn situation_identified_creates_node() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sit_id = Uuid::new_v4();
    project_all(
        &projector,
        &[stored(
            1,
            &Event::System(SystemEvent::SituationIdentified {
                situation_id: sit_id,
                headline: "Housing crisis deepens".into(),
                lede: "Rents climb across metro".into(),
                arc: SituationArc::Emerging,
                temperature: 0.75,
                centroid_lat: Some(44.97),
                centroid_lng: Some(-93.26),
                location_name: Some("Minneapolis".into()),
                sensitivity: SensitivityLevel::General,
                category: Some("housing".into()),
                structured_state: "{}".into(),
                tension_heat: None,
                clarity: None,
                signal_count: Some(3),
                narrative_embedding: None,
                causal_embedding: None,
            }),
        )],
    )
    .await;

    assert_eq!(count_nodes(&client, "Situation").await, 1);
    let headline: String = read_prop(&client, "Situation", sit_id, "headline").await;
    assert_eq!(headline, "Housing crisis deepens");
    let arc: String = read_prop(&client, "Situation", sit_id, "arc").await;
    assert_eq!(arc, "emerging");
    let temp: f64 = read_prop(&client, "Situation", sit_id, "temperature").await;
    assert!((temp - 0.75).abs() < 0.01);
}

#[tokio::test]
async fn signal_assigned_creates_part_of_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sit_id = Uuid::new_v4();
    let sig_id = Uuid::new_v4();

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::SituationIdentified {
                    situation_id: sit_id,
                    headline: "Test Situation".into(),
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
                }),
            ),
            stored(
                2,
                &Event::World(WorldEvent::ConcernRaised {
                    id: sig_id,
                    title: "Noise issue".into(),
                    summary: "".into(),
                    url: "https://x.com/noise".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    subject: None,
                    opposing: None,
                }),
            ),
            stored(
                3,
                &Event::System(SystemEvent::SignalAssignedToSituation {
                    signal_id: sig_id,
                    situation_id: sit_id,
                    signal_label: "Concern".into(),
                    confidence: 0.9,
                    reasoning: "Directly related".into(),
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_edges(&client, sig_id, "PART_OF", "Situation").await, 1);
    let sc: i64 = read_prop(&client, "Situation", sit_id, "signal_count").await;
    assert_eq!(sc, 1);
}

#[tokio::test]
async fn situation_changed_updates_properties() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sit_id = Uuid::new_v4();
    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::SituationIdentified {
                    situation_id: sit_id,
                    headline: "Original headline".into(),
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
                }),
            ),
            stored(
                2,
                &Event::System(SystemEvent::SituationChanged {
                    situation_id: sit_id,
                    change: SituationChange::Headline {
                        old: "Original headline".into(),
                        new: "Updated headline".into(),
                    },
                }),
            ),
            stored(
                3,
                &Event::System(SystemEvent::SituationChanged {
                    situation_id: sit_id,
                    change: SituationChange::Temperature {
                        old: 0.5,
                        new: 0.9,
                    },
                }),
            ),
        ],
    )
    .await;

    let headline: String = read_prop(&client, "Situation", sit_id, "headline").await;
    assert_eq!(headline, "Updated headline");
    let temp: f64 = read_prop(&client, "Situation", sit_id, "temperature").await;
    assert!((temp - 0.9).abs() < 0.01);
}

// ── Replay Idempotency ─────────────────────────────────────────────

#[tokio::test]
async fn signal_assigned_twice_does_not_double_count() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sit_id = Uuid::new_v4();
    let sig_id = Uuid::new_v4();

    let sit_event = stored(
        1,
        &Event::System(SystemEvent::SituationIdentified {
            situation_id: sit_id,
            headline: "Test".into(),
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
        }),
    );

    let sig_event = stored(
        2,
        &Event::World(WorldEvent::ConcernRaised {
            id: sig_id,
            title: "Issue".into(),
            summary: "".into(),
            url: "https://x.com/issue".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            subject: None,
            opposing: None,
        }),
    );

    let assign = Event::System(SystemEvent::SignalAssignedToSituation {
        signal_id: sig_id,
        situation_id: sit_id,
        signal_label: "Concern".into(),
        confidence: 0.9,
        reasoning: "Related".into(),
    });

    // Project once
    project_all(&projector, &[sit_event.clone(), sig_event.clone(), stored(3, &assign)]).await;

    // Project the same assignment again (replay)
    project_all(&projector, &[stored(4, &assign)]).await;

    let sc: i64 = read_prop(&client, "Situation", sit_id, "signal_count").await;
    assert_eq!(sc, 1, "Replaying SignalAssignedToSituation should not double signal_count");
}

#[tokio::test]
async fn dispatch_created_twice_does_not_double_count() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sit_id = Uuid::new_v4();
    let dispatch_id = Uuid::new_v4();

    let sit_event = stored(
        1,
        &Event::System(SystemEvent::SituationIdentified {
            situation_id: sit_id,
            headline: "Test".into(),
            lede: "".into(),
            arc: SituationArc::Active,
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
        }),
    );

    let dispatch = Event::System(SystemEvent::DispatchCreated {
        dispatch_id,
        situation_id: sit_id,
        body: "Update".into(),
        signal_ids: vec![],
        dispatch_type: DispatchType::Update,
        supersedes: None,
        fidelity_score: None,
        flagged_for_review: None,
        flag_reason: None,
    });

    // Project once
    project_all(&projector, &[sit_event.clone(), stored(2, &dispatch)]).await;

    // Project the same dispatch again (replay)
    project_all(&projector, &[stored(3, &dispatch)]).await;

    let dc: i64 = read_prop(&client, "Situation", sit_id, "dispatch_count").await;
    assert_eq!(dc, 1, "Replaying DispatchCreated should not double dispatch_count");
}

#[tokio::test]
async fn observation_corroborated_replay_does_not_double_count() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sig_id = Uuid::new_v4();

    let corroborate = Event::System(SystemEvent::ObservationCorroborated {
        signal_id: sig_id,
        node_type: NodeType::Gathering,
        new_url: "https://x.com/corr".into(),
        summary: None,
    });

    // Create gathering event first so its timestamp is earlier
    let gathering_event = stored(
        1,
        &Event::World(WorldEvent::GatheringAnnounced {
            id: sig_id,
            title: "Rally".into(),
            summary: "".into(),
            url: "https://x.com/rally".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            action_url: None,
        }),
    );
    let corr_event = stored(2, &corroborate);

    project_all(&projector, &[gathering_event, corr_event.clone()]).await;

    // Replay the same PersistedEvent (same created_at, modeling true replay)
    project_all(&projector, &[corr_event]).await;

    let count: i64 = read_prop(&client, "Gathering", sig_id, "corroboration_count").await;
    assert_eq!(count, 1, "Replaying ObservationCorroborated should not double corroboration_count");
}

#[tokio::test]
async fn resource_identified_replay_does_not_double_signal_count() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let res_id = Uuid::new_v4();
    let resource = Event::World(WorldEvent::ResourceIdentified {
        resource_id: res_id,
        name: "Food".into(),
        slug: "food".into(),
        description: "Canned food".into(),
    });

    project_all(&projector, &[stored(1, &resource)]).await;

    // Replay the same identification
    project_all(&projector, &[stored(2, &resource)]).await;

    let sc: i64 = read_prop(&client, "Resource", res_id, "signal_count").await;
    assert_eq!(sc, 1, "Replaying ResourceIdentified should not double signal_count");
}

#[tokio::test]
async fn duplicate_concern_merged_replay_does_not_double_corroboration() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let survivor_id = Uuid::new_v4();
    let dup_id = Uuid::new_v4();

    let merge_event = Event::System(SystemEvent::DuplicateConcernMerged {
        survivor_id,
        duplicate_id: dup_id,
    });

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::ConcernRaised {
                    id: survivor_id,
                    title: "Noise original".into(),
                    summary: "".into(),
                    url: "https://x.com/noise1".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    subject: None,
                    opposing: None,
                }),
            ),
            stored(
                2,
                &Event::World(WorldEvent::ConcernRaised {
                    id: dup_id,
                    title: "Noise duplicate".into(),
                    summary: "".into(),
                    url: "https://x.com/noise2".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    subject: None,
                    opposing: None,
                }),
            ),
            stored(3, &merge_event),
        ],
    )
    .await;

    // Replay the merge
    project_all(&projector, &[stored(4, &merge_event)]).await;

    let count: i64 = read_prop(&client, "Concern", survivor_id, "corroboration_count").await;
    assert_eq!(count, 1, "Replaying DuplicateConcernMerged should not double corroboration_count");
}

// ── Dispatches ─────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_created_with_edges() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sit_id = Uuid::new_v4();
    let dispatch_id = Uuid::new_v4();
    let sig_id = Uuid::new_v4();

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::SituationIdentified {
                    situation_id: sit_id,
                    headline: "Test".into(),
                    lede: "".into(),
                    arc: SituationArc::Active,
                    temperature: 0.8,
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
                }),
            ),
            stored(
                2,
                &Event::World(WorldEvent::ConcernRaised {
                    id: sig_id,
                    title: "Issue".into(),
                    summary: "".into(),
                    url: "https://x.com/issue".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    subject: None,
                    opposing: None,
                }),
            ),
            stored(
                3,
                &Event::System(SystemEvent::DispatchCreated {
                    dispatch_id,
                    situation_id: sit_id,
                    body: "Housing crisis update".into(),
                    signal_ids: vec![sig_id],
                    dispatch_type: DispatchType::Update,
                    supersedes: None,
                    fidelity_score: Some(0.95),
                    flagged_for_review: None,
                    flag_reason: None,
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_nodes(&client, "Dispatch").await, 1);
    assert_eq!(
        count_edges(&client, dispatch_id, "BELONGS_TO", "Situation").await,
        1
    );
    assert_eq!(
        count_edges(&client, dispatch_id, "CITES", "Concern").await,
        1
    );
}

#[tokio::test]
async fn dispatch_supersedes_creates_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sit_id = Uuid::new_v4();
    let old_dispatch = Uuid::new_v4();
    let new_dispatch = Uuid::new_v4();

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::SituationIdentified {
                    situation_id: sit_id,
                    headline: "Test".into(),
                    lede: "".into(),
                    arc: SituationArc::Active,
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
                }),
            ),
            stored(
                2,
                &Event::System(SystemEvent::DispatchCreated {
                    dispatch_id: old_dispatch,
                    situation_id: sit_id,
                    body: "First update".into(),
                    signal_ids: vec![],
                    dispatch_type: DispatchType::Emergence,
                    supersedes: None,
                    fidelity_score: None,
                    flagged_for_review: None,
                    flag_reason: None,
                }),
            ),
            stored(
                3,
                &Event::System(SystemEvent::DispatchCreated {
                    dispatch_id: new_dispatch,
                    situation_id: sit_id,
                    body: "Updated report".into(),
                    signal_ids: vec![],
                    dispatch_type: DispatchType::Update,
                    supersedes: Some(old_dispatch),
                    fidelity_score: None,
                    flagged_for_review: None,
                    flag_reason: None,
                }),
            ),
        ],
    )
    .await;

    assert_eq!(
        count_edges(&client, new_dispatch, "SUPERSEDES", "Dispatch").await,
        1
    );
}

// ── More Replay Idempotency ───────────────────────────────────────

#[tokio::test]
async fn gathering_scouted_miss_replay_does_not_double_count() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let concern_id = Uuid::new_v4();
    let scouted_at = Utc::now();

    let scout = Event::System(SystemEvent::GatheringScouted {
        concern_id,
        found_gatherings: false,
        scouted_at,
    });

    let scout_event = stored(2, &scout);

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::ConcernRaised {
                    id: concern_id,
                    title: "Noise".into(),
                    summary: "".into(),
                    url: "https://x.com/noise".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    subject: None,
                    opposing: None,
                }),
            ),
            scout_event.clone(),
        ],
    )
    .await;

    // Replay the same scout miss
    project_all(&projector, &[scout_event]).await;

    let miss_count: i64 = read_prop(&client, "Concern", concern_id, "gravity_scout_miss_count").await;
    assert_eq!(miss_count, 1, "Replaying GatheringScouted miss should not double gravity_scout_miss_count");
}

#[tokio::test]
async fn concern_linker_retry_replay_does_not_double_count() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sig_id = Uuid::new_v4();

    let outcome = Event::System(SystemEvent::ConcernLinkerOutcomeRecorded {
        signal_id: sig_id,
        label: "Gathering".into(),
        outcome: "failed".into(),
        increment_retry: true,
    });

    let outcome_event = stored(2, &outcome);

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: sig_id,
                    title: "Rally".into(),
                    summary: "".into(),
                    url: "https://x.com/rally".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            outcome_event.clone(),
        ],
    )
    .await;

    // Replay the same outcome
    project_all(&projector, &[outcome_event]).await;

    let retry_count: i64 = read_prop(&client, "Gathering", sig_id, "curiosity_retry_count").await;
    assert_eq!(retry_count, 1, "Replaying ConcernLinkerOutcomeRecorded should not double curiosity_retry_count");
}
