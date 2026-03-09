#[cfg(test)]
mod tests {
    use rootsignal_common::canonical_value;

    use crate::testing::{actor_with_external_url, actor_without_external_url, make_source, MockSignalReader};
    use crate::domains::enrichment::activities::source_claimer;

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
        let fb_source = make_source("https://www.facebook.com/sanctuarysupplydepot", &fb_ck);
        let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

        let store = MockSignalReader::new()
            .add_source(fb_source.clone())
            .add_source(ig_source.clone());

        let actors = vec![(actor.clone(), vec![ig_source])];

        let result = source_claimer::claim_profile_sources(&store, &actors).await.unwrap();

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

        let actors = vec![(actor, vec![ig_source, fb_source])];

        let result = source_claimer::claim_profile_sources(&store, &actors).await.unwrap();

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

        let store = MockSignalReader::new()
            .add_source(ig_source.clone());

        let actors = vec![(actor.clone(), vec![ig_source])];

        let result = source_claimer::claim_profile_sources(&store, &actors).await.unwrap();

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

        let result = source_claimer::claim_profile_sources(&store, &actors).await.unwrap();

        assert!(result.link_events.is_empty());
        assert!(result.new_sources.is_empty());
    }

    #[tokio::test]
    async fn external_url_same_as_own_source_ignored() {
        let actor = actor_with_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
            "https://www.instagram.com/sanctuarysupply",
        );
        let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

        let store = MockSignalReader::new()
            .add_source(ig_source.clone());

        let actors = vec![(actor, vec![ig_source])];

        let result = source_claimer::claim_profile_sources(&store, &actors).await.unwrap();

        assert!(result.link_events.is_empty(), "should not re-link own source");
        assert!(result.new_sources.is_empty());
    }

    #[tokio::test]
    async fn batch_processes_multiple_actors_independently() {
        let actor_a = actor_with_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
            "https://www.sanctuarysupply.org",
        );
        let actor_b = actor_without_external_url(
            "Quiet Collective",
            "instagram.com/quietcollective",
        );
        let actor_c = actor_with_external_url(
            "Midway Mutual Aid",
            "instagram.com/midwaymutualaid",
            "https://www.facebook.com/midwaymutualaid",
        );

        let ig_a = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");
        let ig_b = make_source("https://www.instagram.com/quietcollective", "instagram.com/quietcollective");
        let ig_c = make_source("https://www.instagram.com/midwaymutualaid", "instagram.com/midwaymutualaid");
        let fb_ck = canonical_value("https://www.facebook.com/midwaymutualaid");
        let fb_c = make_source("https://www.facebook.com/midwaymutualaid", &fb_ck);

        let store = MockSignalReader::new()
            .add_source(ig_a.clone())
            .add_source(ig_b.clone())
            .add_source(ig_c.clone())
            .add_source(fb_c.clone());

        let actors = vec![
            (actor_a.clone(), vec![ig_a]),
            (actor_b, vec![ig_b]),
            (actor_c.clone(), vec![ig_c]),
        ];

        let result = source_claimer::claim_profile_sources(&store, &actors).await.unwrap();

        // actor_a: unknown URL → new source + link
        // actor_b: no external_url → skip
        // actor_c: known FB source → link only
        assert_eq!(result.link_events.len(), 2);
        assert_eq!(result.new_sources.len(), 1, "only actor_a's unknown URL becomes a new source");

        let c_event = result.link_events.iter().find(|e| match e {
            rootsignal_common::events::WorldEvent::ActorLinkedToSource { actor_id, .. } => *actor_id == actor_c.id,
            _ => false,
        }).expect("actor_c should have a link event");
        match c_event {
            rootsignal_common::events::WorldEvent::ActorLinkedToSource { source_id, .. } => {
                assert_eq!(*source_id, fb_c.id);
            }
            _ => unreachable!(),
        }
    }
}
