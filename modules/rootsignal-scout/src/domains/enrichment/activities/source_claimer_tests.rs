#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use rootsignal_common::canonical_value;
    use rootsignal_common::types::{ActorNode, ActorType, ProfileSnapshot, SourceNode};

    use crate::testing::{make_source, MockSignalReader};
    use crate::domains::enrichment::activities::source_claimer;

    fn actor_with_external_url(name: &str, canonical_key: &str, external_url: &str) -> ActorNode {
        ActorNode {
            id: Uuid::new_v4(),
            name: name.to_string(),
            actor_type: ActorType::Organization,
            canonical_key: canonical_key.to_string(),
            domains: vec![],
            social_urls: vec![],
            description: String::new(),
            signal_count: 0,
            first_seen: chrono::Utc::now(),
            last_active: chrono::Utc::now(),
            typical_roles: vec![],
            bio: Some("A community org".to_string()),
            external_url: Some(external_url.to_string()),
            location_lat: None,
            location_lng: None,
            location_name: None,
            discovery_depth: 0,
        }
    }

    fn actor_without_external_url(name: &str, canonical_key: &str) -> ActorNode {
        ActorNode {
            id: Uuid::new_v4(),
            name: name.to_string(),
            actor_type: ActorType::Organization,
            canonical_key: canonical_key.to_string(),
            domains: vec![],
            social_urls: vec![],
            description: String::new(),
            signal_count: 0,
            first_seen: chrono::Utc::now(),
            last_active: chrono::Utc::now(),
            typical_roles: vec![],
            bio: Some("A community org".to_string()),
            external_url: None,
            location_lat: None,
            location_lng: None,
            location_name: None,
            discovery_depth: 0,
        }
    }

    // ---------------------------------------------------------------
    // Sub-problem A: external URL matches a known source
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn known_source_gets_linked_to_actor() {
        let actor = actor_with_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
            "https://www.facebook.com/sanctuarysupplydepot",
        );
        let fb_ck = canonical_value("https://www.facebook.com/sanctuarysupplydepot");
        let fb_source = make_source(&format!("https://www.facebook.com/sanctuarysupplydepot"), &fb_ck);
        let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

        let store = MockSignalReader::new()
            .add_source(fb_source.clone())
            .add_source(ig_source.clone());

        // Actor currently only linked to Instagram source
        let actors = vec![(actor.clone(), vec![ig_source])];

        let result = source_claimer::claim_profile_sources(&store, &actors).await;

        assert_eq!(result.link_events.len(), 1, "should link actor to the known Facebook source");
        let event = &result.link_events[0];
        match event {
            rootsignal_common::events::WorldEvent::ActorLinkedToSource { actor_id, source_id } => {
                assert_eq!(*actor_id, actor.id);
                assert_eq!(*source_id, fb_source.id);
            }
            other => panic!("expected ActorLinkedToSource, got {other:?}"),
        }
        assert!(result.new_sources.is_empty(), "no new sources needed");
    }

    #[tokio::test]
    async fn already_linked_source_not_duplicated() {
        let actor = actor_with_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
            "https://www.facebook.com/sanctuarysupplydepot",
        );
        let fb_ck = canonical_value("https://www.facebook.com/sanctuarysupplydepot");
        let fb_source = make_source("https://www.facebook.com/sanctuarysupplydepot", &fb_ck);
        let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

        let store = MockSignalReader::new()
            .add_source(fb_source.clone())
            .add_source(ig_source.clone());

        // Actor already linked to BOTH sources
        let actors = vec![(actor, vec![ig_source, fb_source])];

        let result = source_claimer::claim_profile_sources(&store, &actors).await;

        assert!(result.link_events.is_empty(), "should not re-link an already-linked source");
        assert!(result.new_sources.is_empty());
    }

    // ---------------------------------------------------------------
    // Sub-problem B: external URL is an unknown website
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn unknown_url_promoted_as_new_source() {
        let actor = actor_with_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
            "https://www.sanctuarysupply.org",
        );
        let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

        // No source exists for sanctuarysupply.org
        let store = MockSignalReader::new()
            .add_source(ig_source.clone());

        let actors = vec![(actor.clone(), vec![ig_source])];

        let result = source_claimer::claim_profile_sources(&store, &actors).await;

        assert_eq!(result.new_sources.len(), 1, "should promote external URL as new source");
        let new_source = &result.new_sources[0];
        assert_eq!(new_source.canonical_key, canonical_value("https://www.sanctuarysupply.org"));

        assert_eq!(result.link_events.len(), 1, "should link actor to the new source");
        match &result.link_events[0] {
            rootsignal_common::events::WorldEvent::ActorLinkedToSource { actor_id, source_id } => {
                assert_eq!(*actor_id, actor.id);
                assert_eq!(*source_id, new_source.id);
            }
            other => panic!("expected ActorLinkedToSource, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Edge cases
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn no_external_url_no_action() {
        let actor = actor_without_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
        );
        let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

        let store = MockSignalReader::new()
            .add_source(ig_source.clone());

        let actors = vec![(actor, vec![ig_source])];

        let result = source_claimer::claim_profile_sources(&store, &actors).await;

        assert!(result.link_events.is_empty());
        assert!(result.new_sources.is_empty());
    }

    #[tokio::test]
    async fn external_url_same_as_own_source_ignored() {
        // Actor's external_url points to their own Instagram — nothing to claim
        let actor = actor_with_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
            "https://www.instagram.com/sanctuarysupply",
        );
        let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

        let store = MockSignalReader::new()
            .add_source(ig_source.clone());

        let actors = vec![(actor, vec![ig_source])];

        let result = source_claimer::claim_profile_sources(&store, &actors).await;

        assert!(result.link_events.is_empty(), "should not re-link own source");
        assert!(result.new_sources.is_empty());
    }
}
