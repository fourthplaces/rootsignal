use uuid::Uuid;

use rootsignal_common::events::{Event, SystemEvent, WorldEvent};
use rootsignal_common::{ActorType, DiscoveryMethod, SourceNode, SourceRole};
use rootsignal_graph::GraphProjector;
use rootsignal_world::types::{Entity, EntityType};

use super::helpers::*;

// ── Source Linking ──────────────────────────────────────────────────

#[tokio::test]
async fn signal_linked_to_source_creates_produced_by_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sig_id = Uuid::new_v4();
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
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: sig_id,
                    title: "Block Party".into(),
                    summary: "Neighborhood block party".into(),
                    url: "https://example.com/party".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![mpls()],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            stored(
                2,
                &Event::System(SystemEvent::SourcesRegistered {
                    sources: vec![source],
                }),
            ),
            stored(
                3,
                &Event::World(WorldEvent::SignalLinkedToSource {
                    signal_id: sig_id,
                    source_id: src_id,
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_edges(&client, sig_id, "PRODUCED_BY", "Source").await, 1);
}

#[tokio::test]
async fn same_link_twice_is_idempotent() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sig_id = Uuid::new_v4();
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

    let link = Event::World(WorldEvent::SignalLinkedToSource {
        signal_id: sig_id,
        source_id: src_id,
    });

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: sig_id,
                    title: "Party".into(),
                    summary: "".into(),
                    url: "https://example.com/p".into(),
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
                &Event::System(SystemEvent::SourcesRegistered {
                    sources: vec![source],
                }),
            ),
            stored(3, &link),
            stored(4, &link),
        ],
    )
    .await;

    assert_eq!(count_edges(&client, sig_id, "PRODUCED_BY", "Source").await, 1);
}

#[tokio::test]
async fn signal_linked_to_multiple_sources() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sig_id = Uuid::new_v4();
    let src_a = SourceNode::new(
        "https://a.com".into(),
        "a.com".into(),
        None,
        DiscoveryMethod::Curated,
        0.5,
        SourceRole::Mixed,
        None,
    );
    let src_b = SourceNode::new(
        "https://b.com".into(),
        "b.com".into(),
        None,
        DiscoveryMethod::Curated,
        0.5,
        SourceRole::Mixed,
        None,
    );
    let a_id = src_a.id;
    let b_id = src_b.id;

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: sig_id,
                    title: "Party".into(),
                    summary: "".into(),
                    url: "https://a.com/p".into(),
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
                &Event::System(SystemEvent::SourcesRegistered {
                    sources: vec![src_a, src_b],
                }),
            ),
            stored(
                3,
                &Event::World(WorldEvent::SignalLinkedToSource {
                    signal_id: sig_id,
                    source_id: a_id,
                }),
            ),
            stored(
                4,
                &Event::World(WorldEvent::SignalLinkedToSource {
                    signal_id: sig_id,
                    source_id: b_id,
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_edges(&client, sig_id, "PRODUCED_BY", "Source").await, 2);
}

// ── Location Edges ─────────────────────────────────────────────────

#[tokio::test]
async fn gathering_with_location_creates_held_at_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let id = Uuid::new_v4();
    project_all(
        &projector,
        &[stored(
            1,
            &Event::World(WorldEvent::GatheringAnnounced {
                id,
                title: "Potluck".into(),
                summary: "".into(),
                url: "https://x.com/p".into(),
                published_at: None,
                extraction_id: None,
                locations: vec![loc("Powderhorn Park", 44.934, -93.234)],
                mentioned_entities: vec![],
                references: vec![],
                schedule: None,
                action_url: None,
            }),
        )],
    )
    .await;

    assert_eq!(count_edges(&client, id, "HELD_AT", "Location").await, 1);
    assert_eq!(count_nodes(&client, "Location").await, 1);
}

#[tokio::test]
async fn each_signal_type_creates_its_own_location_edge_type() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    let location = vec![loc("City Hall", 44.977, -93.265)];

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::ResourceOffered {
                    id: ids[0],
                    title: "Food Shelf".into(),
                    summary: "".into(),
                    url: "https://x.com/r".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: location.clone(),
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                    availability: None,
                    eligibility: None,
                }),
            ),
            stored(
                2,
                &Event::World(WorldEvent::HelpRequested {
                    id: ids[1],
                    title: "Volunteers".into(),
                    summary: "".into(),
                    url: "https://x.com/h".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: location.clone(),
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    what_needed: None,
                    stated_goal: None,
                }),
            ),
            stored(
                3,
                &Event::World(WorldEvent::AnnouncementShared {
                    id: ids[2],
                    title: "Notice".into(),
                    summary: "".into(),
                    url: "https://x.com/a".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: location.clone(),
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    subject: None,
                    effective_date: None,
                }),
            ),
            stored(
                4,
                &Event::World(WorldEvent::ConcernRaised {
                    id: ids[3],
                    title: "Noise".into(),
                    summary: "".into(),
                    url: "https://x.com/c".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: location.clone(),
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    subject: None,
                    opposing: None,
                }),
            ),
            stored(
                5,
                &Event::World(WorldEvent::ConditionObserved {
                    id: ids[4],
                    title: "Air Quality".into(),
                    summary: "".into(),
                    url: "https://x.com/co".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: location.clone(),
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    subject: None,
                    observed_by: None,
                    measurement: None,
                    affected_scope: None,
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_edges(&client, ids[0], "AVAILABLE_AT", "Location").await, 1);
    assert_eq!(count_edges(&client, ids[1], "NEEDED_AT", "Location").await, 1);
    assert_eq!(count_edges(&client, ids[2], "RELEVANT_TO", "Location").await, 1);
    assert_eq!(count_edges(&client, ids[3], "AFFECTS", "Location").await, 1);
    assert_eq!(count_edges(&client, ids[4], "OBSERVED_AT", "Location").await, 1);
    assert_eq!(count_nodes(&client, "Location").await, 1);
}

#[tokio::test]
async fn signal_with_multiple_locations_creates_multiple_edges() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let id = Uuid::new_v4();
    project_all(
        &projector,
        &[stored(
            1,
            &Event::World(WorldEvent::GatheringAnnounced {
                id,
                title: "Tour".into(),
                summary: "".into(),
                url: "https://x.com/t".into(),
                published_at: None,
                extraction_id: None,
                locations: vec![
                    loc("Stop A", 44.930, -93.230),
                    loc("Stop B", 44.970, -93.220),
                ],
                mentioned_entities: vec![],
                references: vec![],
                schedule: None,
                action_url: None,
            }),
        )],
    )
    .await;

    assert_eq!(count_edges(&client, id, "HELD_AT", "Location").await, 2);
    assert_eq!(count_nodes(&client, "Location").await, 2);
}

#[tokio::test]
async fn signal_with_no_location_creates_no_location_node() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let id = Uuid::new_v4();
    project_all(
        &projector,
        &[stored(
            1,
            &Event::World(WorldEvent::GatheringAnnounced {
                id,
                title: "Online".into(),
                summary: "".into(),
                url: "https://x.com/o".into(),
                published_at: None,
                extraction_id: None,
                locations: vec![],
                mentioned_entities: vec![],
                references: vec![],
                schedule: None,
                action_url: None,
            }),
        )],
    )
    .await;

    assert_eq!(count_nodes(&client, "Location").await, 0);
}

#[tokio::test]
async fn same_location_shared_by_two_signals_creates_one_node() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let sig1 = Uuid::new_v4();
    let sig2 = Uuid::new_v4();
    let shared_loc = loc("Powderhorn Park", 44.934, -93.234);

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: sig1,
                    title: "Potluck".into(),
                    summary: "".into(),
                    url: "https://x.com/1".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![shared_loc.clone()],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                }),
            ),
            stored(
                2,
                &Event::World(WorldEvent::ResourceOffered {
                    id: sig2,
                    title: "Free food".into(),
                    summary: "".into(),
                    url: "https://x.com/2".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![shared_loc],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                    availability: None,
                    eligibility: None,
                }),
            ),
        ],
    )
    .await;

    assert_eq!(count_nodes(&client, "Location").await, 1);
    assert_eq!(count_edges(&client, sig1, "HELD_AT", "Location").await, 1);
    assert_eq!(count_edges(&client, sig2, "AVAILABLE_AT", "Location").await, 1);
}

#[tokio::test]
async fn location_role_overrides_default_edge_type() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let id = Uuid::new_v4();
    project_all(
        &projector,
        &[stored(
            1,
            &Event::World(WorldEvent::GatheringAnnounced {
                id,
                title: "Response Event".into(),
                summary: "".into(),
                url: "https://x.com/re".into(),
                published_at: None,
                extraction_id: None,
                locations: vec![loc_with_role("Flood Zone", 44.934, -93.234, "affected_area")],
                mentioned_entities: vec![],
                references: vec![],
                schedule: None,
                action_url: None,
            }),
        )],
    )
    .await;

    assert_eq!(count_edges(&client, id, "AFFECTS", "Location").await, 1);
    assert_eq!(count_edges(&client, id, "HELD_AT", "Location").await, 0);
}

// ── Entity Linking (MENTIONED_IN) ──────────────────────────────────

#[tokio::test]
async fn signal_with_entities_creates_mentioned_in_edges() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let id = Uuid::new_v4();
    project_all(
        &projector,
        &[stored(
            1,
            &Event::World(WorldEvent::GatheringAnnounced {
                id,
                title: "City Council Meeting".into(),
                summary: "".into(),
                url: "https://x.com/cc".into(),
                published_at: None,
                extraction_id: None,
                locations: vec![],
                mentioned_entities: vec![
                    Entity {
                        name: "City Council".into(),
                        entity_type: EntityType::GovernmentBody,
                        role: Some("organizer".into()),
                    },
                    Entity {
                        name: "Parks Department".into(),
                        entity_type: EntityType::GovernmentBody,
                        role: Some("presenter".into()),
                    },
                ],
                references: vec![],
                schedule: None,
                action_url: None,
            }),
        )],
    )
    .await;

    assert_eq!(count_nodes(&client, "Entity").await, 2);
    assert_eq!(count_edges(&client, id, "MENTIONED_IN", "Gathering").await, 0);

    // MENTIONED_IN goes Entity→Signal, so count inbound edges
    let cypher = "MATCH (e:Entity)-[:MENTIONED_IN]->(n:Gathering {id: $id}) RETURN count(e) AS cnt";
    let q = rootsignal_graph::query(cypher).param("id", id.to_string());
    let mut stream = client.execute(q).await.expect("query failed");
    let cnt: i64 = stream
        .next()
        .await
        .expect("stream failed")
        .map(|r| r.get::<i64>("cnt").unwrap_or(0))
        .unwrap_or(0);
    assert_eq!(cnt, 2);
}

#[tokio::test]
async fn entity_matching_actor_creates_same_as_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let actor_id = Uuid::new_v4();
    let sig_id = Uuid::new_v4();

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::System(SystemEvent::ActorIdentified {
                    actor_id,
                    name: "Housing Alliance".into(),
                    actor_type: ActorType::Organization,
                    canonical_key: "housing-alliance".into(),
                    domains: vec![],
                    social_urls: vec![],
                    description: "Housing org".into(),
                    bio: None,
                    location_lat: None,
                    location_lng: None,
                    location_name: None,
                }),
            ),
            stored(
                2,
                &Event::World(WorldEvent::ConcernRaised {
                    id: sig_id,
                    title: "Rent crisis".into(),
                    summary: "".into(),
                    url: "https://x.com/rent".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![Entity {
                        name: "Housing Alliance".into(),
                        entity_type: EntityType::Organization,
                        role: Some("subject".into()),
                    }],
                    references: vec![],
                    schedule: None,
                    subject: None,
                    opposing: None,
                }),
            ),
        ],
    )
    .await;

    let cypher = "MATCH (e:Entity)-[:SAME_AS]->(a:Actor) RETURN count(*) AS cnt";
    let q = rootsignal_graph::query(cypher);
    let mut stream = client.execute(q).await.expect("query failed");
    let cnt: i64 = stream
        .next()
        .await
        .expect("stream failed")
        .map(|r| r.get::<i64>("cnt").unwrap_or(0))
        .unwrap_or(0);
    assert_eq!(cnt, 1);
}

// ── Response/Concern Linking ───────────────────────────────────────

#[tokio::test]
async fn response_linked_creates_responds_to_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let concern_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::ConcernRaised {
                    id: concern_id,
                    title: "Housing shortage".into(),
                    summary: "".into(),
                    url: "https://x.com/concern".into(),
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
                &Event::World(WorldEvent::ResourceOffered {
                    id: resource_id,
                    title: "Rent assistance".into(),
                    summary: "".into(),
                    url: "https://x.com/resource".into(),
                    published_at: None,
                    extraction_id: None,
                    locations: vec![],
                    mentioned_entities: vec![],
                    references: vec![],
                    schedule: None,
                    action_url: None,
                    availability: None,
                    eligibility: None,
                }),
            ),
            stored(
                3,
                &Event::System(SystemEvent::ResponseLinked {
                    signal_id: resource_id,
                    concern_id,
                    strength: 0.85,
                    explanation: "Direct match".into(),
                    source_url: None,
                }),
            ),
        ],
    )
    .await;

    assert_eq!(
        count_edges(&client, resource_id, "RESPONDS_TO", "Concern").await,
        1
    );

    let cypher = "MATCH ()-[r:RESPONDS_TO]->() RETURN r.match_strength AS val LIMIT 1";
    let q = rootsignal_graph::query(cypher);
    let mut stream = client.execute(q).await.expect("query");
    let strength: f64 = stream
        .next()
        .await
        .expect("stream")
        .map(|r| r.get::<f64>("val").unwrap_or(0.0))
        .unwrap_or(0.0);
    assert!((strength - 0.85).abs() < 0.01);
}

#[tokio::test]
async fn concern_linked_creates_drawn_to_edge() {
    let (_guard, client) = super::setup().await;
    let projector = GraphProjector::new(client.clone());

    let concern_id = Uuid::new_v4();
    let gathering_id = Uuid::new_v4();

    project_all(
        &projector,
        &[
            stored(
                1,
                &Event::World(WorldEvent::ConcernRaised {
                    id: concern_id,
                    title: "Noise complaints".into(),
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
                2,
                &Event::World(WorldEvent::GatheringAnnounced {
                    id: gathering_id,
                    title: "Block party".into(),
                    summary: "".into(),
                    url: "https://x.com/party".into(),
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
                &Event::System(SystemEvent::ConcernLinked {
                    signal_id: gathering_id,
                    concern_id,
                    strength: 0.7,
                    explanation: "Related topic".into(),
                    source_url: None,
                }),
            ),
        ],
    )
    .await;

    assert_eq!(
        count_edges(&client, gathering_id, "DRAWN_TO", "Concern").await,
        1
    );
}
