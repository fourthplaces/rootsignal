//! Coalescing domain tests.
//!
//! MOCK → FUNCTION → OUTPUT throughout.
//! Tests exercise the handler via seesaw engine (weave topology),
//! tools via MockGraphQueries, and serde round-trips for new events.

#[cfg(test)]
mod handler_tests {
    use std::sync::{Arc, Mutex};

    use uuid::Uuid;

    use rootsignal_graph::GraphQueries;
    use seesaw_core::AnyEvent;

    use crate::core::engine::{build_weave_engine, ScoutEngineDeps};
    use crate::domains::coalescing::events::CoalescingEvent;
    use crate::domains::lifecycle::events::LifecycleEvent;
    use crate::testing::{FixedEmbedder, MockGraphQueries, MockSignalReader, TEST_EMBEDDING_DIM};
    use crate::traits::SignalReader;

    fn weave_engine_with_capture(
        graph: Option<Arc<dyn GraphQueries>>,
        ai: Option<Arc<dyn ai_client::Agent>>,
    ) -> (seesaw_core::Engine<ScoutEngineDeps>, Arc<Mutex<Vec<AnyEvent>>>) {
        let store = Arc::new(MockSignalReader::new());
        let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));
        let run_id = Uuid::new_v4();
        let captured = Arc::new(Mutex::new(Vec::new()));

        let mut deps = ScoutEngineDeps::new(
            store as Arc<dyn SignalReader>,
            embedder,
            run_id,
        );
        deps.graph = graph;
        deps.ai = ai;
        deps.captured_events = Some(captured.clone());

        let engine = build_weave_engine(deps, None);
        (engine, captured)
    }

    fn has_coalescing_skipped(captured: &[AnyEvent]) -> bool {
        captured.iter().any(|e| {
            e.downcast_ref::<CoalescingEvent>()
                .is_some_and(|ce| matches!(ce, CoalescingEvent::CoalescingSkipped { .. }))
        })
    }

    fn has_coalescing_completed(captured: &[AnyEvent]) -> bool {
        captured.iter().any(|e| {
            e.downcast_ref::<CoalescingEvent>()
                .is_some_and(|ce| matches!(ce, CoalescingEvent::CoalescingCompleted { .. }))
        })
    }

    fn get_coalescing_skipped_reason(captured: &[AnyEvent]) -> Option<String> {
        captured.iter().find_map(|e| {
            e.downcast_ref::<CoalescingEvent>().and_then(|ce| match ce {
                CoalescingEvent::CoalescingSkipped { reason } => Some(reason.clone()),
                _ => None,
            })
        })
    }

    fn get_coalescing_completed(captured: &[AnyEvent]) -> Option<(u32, u32, u32)> {
        captured.iter().find_map(|e| {
            e.downcast_ref::<CoalescingEvent>().and_then(|ce| match ce {
                CoalescingEvent::CoalescingCompleted {
                    new_groups,
                    fed_signals,
                    refined_groups,
                } => Some((*new_groups, *fed_signals, *refined_groups)),
                _ => None,
            })
        })
    }

    #[tokio::test]
    async fn missing_graph_dep_skips_coalescing() {
        let (engine, captured) = weave_engine_with_capture(None, None);

        engine
            .emit(LifecycleEvent::GenerateSituationsRequested {
                run_id: Uuid::new_v4(),
                region: rootsignal_common::ScoutScope {
                    center_lat: 44.97,
                    center_lng: -93.26,
                    radius_km: 50.0,
                    name: "Minneapolis".into(),
                },
                budget_cents: 100,
                region_id: None,
                task_id: None,
            })
            .settled()
            .await
            .unwrap();

        let events = captured.lock().unwrap().clone();
        assert!(
            has_coalescing_skipped(&events),
            "Should skip when graph dep is missing"
        );
        assert_eq!(
            get_coalescing_skipped_reason(&events).as_deref(),
            Some("missing graph or AI deps")
        );
    }

    #[tokio::test]
    async fn missing_ai_dep_skips_coalescing() {
        let graph = Arc::new(MockGraphQueries::new()) as Arc<dyn GraphQueries>;
        let (engine, captured) = weave_engine_with_capture(Some(graph), None);

        engine
            .emit(LifecycleEvent::GenerateSituationsRequested {
                run_id: Uuid::new_v4(),
                region: rootsignal_common::ScoutScope {
                    center_lat: 44.97,
                    center_lng: -93.26,
                    radius_km: 50.0,
                    name: "Minneapolis".into(),
                },
                budget_cents: 100,
                region_id: None,
                task_id: None,
            })
            .settled()
            .await
            .unwrap();

        let events = captured.lock().unwrap().clone();
        assert!(
            has_coalescing_skipped(&events),
            "Should skip when AI dep is missing"
        );
    }

    #[tokio::test]
    async fn insufficient_budget_skips_coalescing() {
        use crate::testing::MockAgent;

        let graph = Arc::new(MockGraphQueries::new()) as Arc<dyn GraphQueries>;
        let ai = Arc::new(MockAgent::with_response(serde_json::json!({}))) as Arc<dyn ai_client::Agent>;
        let (engine, captured) = weave_engine_with_capture(Some(graph), Some(ai));

        // Budget of 1 cent, coalescing costs 15 — should skip
        engine
            .emit(LifecycleEvent::GenerateSituationsRequested {
                run_id: Uuid::new_v4(),
                region: rootsignal_common::ScoutScope {
                    center_lat: 44.97,
                    center_lng: -93.26,
                    radius_km: 50.0,
                    name: "Minneapolis".into(),
                },
                budget_cents: 1,
                region_id: None,
                task_id: None,
            })
            .settled()
            .await
            .unwrap();

        let events = captured.lock().unwrap().clone();
        assert!(
            has_coalescing_skipped(&events),
            "Should skip when budget is insufficient"
        );
        assert_eq!(
            get_coalescing_skipped_reason(&events).as_deref(),
            Some("insufficient budget")
        );
    }

    #[tokio::test]
    async fn stub_coalescer_emits_completed_with_zero_counts() {
        use crate::testing::MockAgent;

        let graph = Arc::new(MockGraphQueries::new()) as Arc<dyn GraphQueries>;
        let ai = Arc::new(MockAgent::with_response(serde_json::json!({}))) as Arc<dyn ai_client::Agent>;
        // budget_cents=0 means unlimited
        let (engine, captured) = weave_engine_with_capture(Some(graph), Some(ai));

        engine
            .emit(LifecycleEvent::GenerateSituationsRequested {
                run_id: Uuid::new_v4(),
                region: rootsignal_common::ScoutScope {
                    center_lat: 44.97,
                    center_lng: -93.26,
                    radius_km: 50.0,
                    name: "Minneapolis".into(),
                },
                budget_cents: 0,
                region_id: None,
                task_id: None,
            })
            .settled()
            .await
            .unwrap();

        let events = captured.lock().unwrap().clone();
        assert!(
            has_coalescing_completed(&events),
            "Stub coalescer should emit CoalescingCompleted"
        );
        assert_eq!(
            get_coalescing_completed(&events),
            Some((0, 0, 0)),
            "Stub should produce zero groups, zero fed signals, zero refined"
        );
    }

    #[tokio::test]
    async fn non_trivial_result_emits_system_events_through_engine() {
        use crate::testing::MockAgent;
        use rootsignal_common::events::SystemEvent;
        use rootsignal_graph::{GroupBrief, SignalDetail, SignalSearchResult};

        let group_id = Uuid::new_v4();
        let new_signal = Uuid::new_v4();

        let graph = MockGraphQueries::new()
            .with_group_landscape(vec![GroupBrief {
                id: group_id,
                label: "Housing issues".into(),
                queries: vec!["rent increase".into()],
                signal_count: 2,
                member_ids: vec![],
            }])
            .with_search_results(vec![SignalSearchResult {
                id: new_signal,
                title: "New rent concern".into(),
                summary: "Rents rising in Uptown".into(),
                signal_type: "Concern".into(),
                score: 0.9,
            }])
            .with_signal_details(vec![SignalDetail {
                id: new_signal,
                title: "New rent concern".into(),
                summary: "Rents rising in Uptown".into(),
                signal_type: "Concern".into(),
                cause_heat: Some(0.7),
            }]);

        let ai = MockAgent::with_response(serde_json::json!({
            "add": [{ "signal_id": new_signal.to_string(), "confidence": 0.88 }],
            "refined_queries": ["rent increase uptown", "housing affordability"]
        }));

        let graph = Arc::new(graph) as Arc<dyn GraphQueries>;
        let ai = Arc::new(ai) as Arc<dyn ai_client::Agent>;
        let (engine, captured) = weave_engine_with_capture(Some(graph), Some(ai));

        engine
            .emit(LifecycleEvent::GenerateSituationsRequested {
                run_id: Uuid::new_v4(),
                region: rootsignal_common::ScoutScope {
                    center_lat: 44.97,
                    center_lng: -93.26,
                    radius_km: 50.0,
                    name: "Minneapolis".into(),
                },
                budget_cents: 0,
                region_id: None,
                task_id: None,
            })
            .settled()
            .await
            .unwrap();

        let events = captured.lock().unwrap().clone();

        let signal_added = events.iter().any(|e| {
            e.downcast_ref::<SystemEvent>().is_some_and(|se| {
                matches!(se, SystemEvent::SignalAddedToGroup { signal_id, group_id: gid, .. }
                    if *signal_id == new_signal && *gid == group_id)
            })
        });
        assert!(signal_added, "Should emit SignalAddedToGroup for the new signal");

        let queries_refined = events.iter().any(|e| {
            e.downcast_ref::<SystemEvent>().is_some_and(|se| {
                matches!(se, SystemEvent::GroupQueriesRefined { group_id: gid, queries }
                    if *gid == group_id && queries.len() == 2)
            })
        });
        assert!(queries_refined, "Should emit GroupQueriesRefined with updated queries");

        let (new_groups, fed_signals, refined_groups) =
            get_coalescing_completed(&events).expect("Should emit CoalescingCompleted");
        assert_eq!(new_groups, 0, "No new groups in feed mode");
        assert_eq!(fed_signals, 1, "One signal fed");
        assert_eq!(refined_groups, 1, "One group's queries refined");
    }
}

#[cfg(test)]
mod serde_tests {
    use rootsignal_common::events::{Event, SystemEvent};
    use uuid::Uuid;

    fn round_trip(event: SystemEvent) {
        let wrapped = Event::System(event);
        let payload = wrapped.to_payload();
        let parsed = Event::from_payload(&payload)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", payload));
        assert_eq!(wrapped.event_type(), parsed.event_type());
    }

    #[test]
    fn group_created_round_trips() {
        round_trip(SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: "Housing affordability".into(),
            queries: vec!["rent increase".into(), "eviction notice".into()],
            seed_signal_id: Some(Uuid::new_v4()),
        });
    }

    #[test]
    fn group_created_without_seed_round_trips() {
        round_trip(SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: "Transit disruptions".into(),
            queries: vec!["bus route change".into()],
            seed_signal_id: None,
        });
    }

    #[test]
    fn group_created_with_empty_queries_round_trips() {
        round_trip(SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: "".into(),
            queries: vec![],
            seed_signal_id: None,
        });
    }

    #[test]
    fn signal_added_to_group_round_trips() {
        round_trip(SystemEvent::SignalAddedToGroup {
            signal_id: Uuid::new_v4(),
            group_id: Uuid::new_v4(),
            confidence: 0.92,
        });
    }

    #[test]
    fn signal_added_to_group_zero_confidence_round_trips() {
        round_trip(SystemEvent::SignalAddedToGroup {
            signal_id: Uuid::new_v4(),
            group_id: Uuid::new_v4(),
            confidence: 0.0,
        });
    }

    #[test]
    fn signal_added_to_group_max_confidence_round_trips() {
        round_trip(SystemEvent::SignalAddedToGroup {
            signal_id: Uuid::new_v4(),
            group_id: Uuid::new_v4(),
            confidence: 1.0,
        });
    }

    #[test]
    fn group_queries_refined_round_trips() {
        round_trip(SystemEvent::GroupQueriesRefined {
            group_id: Uuid::new_v4(),
            queries: vec!["updated query".into()],
        });
    }

    #[test]
    fn group_queries_refined_empty_queries_round_trips() {
        round_trip(SystemEvent::GroupQueriesRefined {
            group_id: Uuid::new_v4(),
            queries: vec![],
        });
    }

    // --- Adversarial serde: try to break serialization ---

    #[test]
    fn unicode_in_group_label_round_trips() {
        round_trip(SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: "住宅問題 🏠 إسكان Wohnungsnot".into(),
            queries: vec!["مساكن".into(), "住宅".into(), "Wohnung".into()],
            seed_signal_id: None,
        });
    }

    #[test]
    fn empty_string_label_round_trips() {
        round_trip(SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: "".into(),
            queries: vec!["".into(), "".into()],
            seed_signal_id: Some(Uuid::new_v4()),
        });
    }

    #[test]
    fn special_chars_in_queries_round_trip() {
        round_trip(SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: r#"He said "hello" & <goodbye>"#.into(),
            queries: vec![
                "query with\nnewline".into(),
                "query with\ttab".into(),
                r#"query with "quotes""#.into(),
                "query with \\backslash".into(),
            ],
            seed_signal_id: None,
        });
    }

    #[test]
    fn very_long_label_round_trips() {
        let long_label = "a".repeat(10_000);
        round_trip(SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: long_label,
            queries: vec!["b".repeat(10_000)],
            seed_signal_id: None,
        });
    }

    #[test]
    fn confidence_boundary_values_round_trip() {
        for confidence in [0.0, 1.0, f64::MIN_POSITIVE, 0.999999999999] {
            round_trip(SystemEvent::SignalAddedToGroup {
                signal_id: Uuid::new_v4(),
                group_id: Uuid::new_v4(),
                confidence,
            });
        }
    }

    #[test]
    fn nil_uuid_round_trips() {
        round_trip(SystemEvent::GroupCreated {
            group_id: Uuid::nil(),
            label: "nil group".into(),
            queries: vec![],
            seed_signal_id: Some(Uuid::nil()),
        });
        round_trip(SystemEvent::SignalAddedToGroup {
            signal_id: Uuid::nil(),
            group_id: Uuid::nil(),
            confidence: 0.5,
        });
    }

    #[test]
    fn deserialization_from_legacy_json_without_seed_field() {
        // Simulate JSON from before seed_signal_id existed — serde tag is unprefixed
        let json = serde_json::json!({
            "type": "group_created",
            "group_id": Uuid::new_v4().to_string(),
            "label": "Legacy group",
            "queries": ["old query"]
        });
        let event: Result<Event, _> = serde_json::from_value(json);
        assert!(event.is_ok(), "Should deserialize without seed_signal_id field: {:?}", event.err());
    }
}

#[cfg(test)]
mod tool_tests {
    use std::sync::Arc;

    use uuid::Uuid;

    use rootsignal_graph::GraphQueries;

    use crate::domains::coalescing::activities::tools::{
        FindSimilarTool, SearchSignalsTool, SearchSignalsArgs, FindSimilarArgs,
    };
    use crate::testing::{FixedEmbedder, MockGraphQueries, TEST_EMBEDDING_DIM};
    use ai_client::tool::Tool;

    #[tokio::test]
    async fn search_signals_returns_empty_for_no_matches() {
        let graph = Arc::new(MockGraphQueries::new()) as Arc<dyn GraphQueries>;
        let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

        let tool = SearchSignalsTool { graph, embedder };
        let result = tool.call(SearchSignalsArgs {
            query: "nonexistent topic".into(),
        }).await.unwrap();

        assert!(result.results.is_empty(), "Empty graph should return no results");
    }

    #[tokio::test]
    async fn find_similar_rejects_invalid_uuid() {
        let graph = Arc::new(MockGraphQueries::new()) as Arc<dyn GraphQueries>;
        let tool = FindSimilarTool { graph };

        let result = tool.call(FindSimilarArgs {
            signal_id: "not-a-uuid".into(),
        }).await;

        assert!(result.is_err(), "Invalid UUID should error");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Invalid signal ID"),
            "Error should mention invalid ID, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn find_similar_errors_for_nonexistent_signal() {
        let graph = Arc::new(MockGraphQueries::new()) as Arc<dyn GraphQueries>;
        let tool = FindSimilarTool { graph };

        let result = tool.call(FindSimilarArgs {
            signal_id: Uuid::new_v4().to_string(),
        }).await;

        assert!(result.is_err(), "Nonexistent signal should error");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "Error should mention signal not found, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn search_signals_falls_back_to_vector_when_few_fulltext_results() {
        let v1 = Uuid::new_v4();
        let v2 = Uuid::new_v4();
        let v3 = Uuid::new_v4();

        let graph = MockGraphQueries::new()
            .with_search_results(vec![
                rootsignal_graph::SignalSearchResult {
                    id: Uuid::new_v4(),
                    title: "FT1".into(),
                    summary: "fulltext hit".into(),
                    signal_type: "Concern".into(),
                    score: 0.9,
                },
                rootsignal_graph::SignalSearchResult {
                    id: Uuid::new_v4(),
                    title: "FT2".into(),
                    summary: "fulltext hit".into(),
                    signal_type: "Concern".into(),
                    score: 0.8,
                },
            ])
            .with_vector_search_results(vec![
                rootsignal_graph::SignalSearchResult {
                    id: v1,
                    title: "Vec1".into(),
                    summary: "vector hit".into(),
                    signal_type: "Concern".into(),
                    score: 0.95,
                },
                rootsignal_graph::SignalSearchResult {
                    id: v2,
                    title: "Vec2".into(),
                    summary: "vector hit".into(),
                    signal_type: "Concern".into(),
                    score: 0.85,
                },
                rootsignal_graph::SignalSearchResult {
                    id: v3,
                    title: "Vec3".into(),
                    summary: "vector hit".into(),
                    signal_type: "Concern".into(),
                    score: 0.75,
                },
            ]);

        let graph = Arc::new(graph) as Arc<dyn GraphQueries>;
        let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

        let tool = SearchSignalsTool { graph, embedder };
        let result = tool.call(SearchSignalsArgs {
            query: "community concern".into(),
        }).await.unwrap();

        assert_eq!(result.results.len(), 3, "Should use vector results when fulltext < 3");
        let ids: Vec<Uuid> = result.results.iter().map(|r| r.id.parse().unwrap()).collect();
        assert!(ids.contains(&v1));
        assert!(ids.contains(&v2));
        assert!(ids.contains(&v3));
    }
}

#[cfg(test)]
mod handler_event_mapping_tests {
    //! Verify the production result_to_events() correctly maps
    //! CoalescingResult → SystemEvents + CoalescingCompleted.

    use uuid::Uuid;

    use rootsignal_common::events::SystemEvent;

    use crate::domains::coalescing::activities::types::{CoalescingResult, FedSignal, ProtoGroup};
    use crate::domains::coalescing::result_to_events;

    #[test]
    fn empty_result_produces_no_system_events() {
        let result = CoalescingResult {
            new_groups: vec![],
            fed_signals: vec![],
            refined_queries: vec![],
        };
        let (system_events, _completed) = result_to_events(&result);
        assert!(system_events.is_empty());
    }

    #[test]
    fn single_signal_group_emits_group_created_only() {
        let group_id = Uuid::new_v4();
        let seed_id = Uuid::new_v4();
        let result = CoalescingResult {
            new_groups: vec![ProtoGroup {
                group_id,
                label: "Test group".into(),
                queries: vec!["test query".into()],
                signal_ids: vec![(seed_id, 1.0)],
            }],
            fed_signals: vec![],
            refined_queries: vec![],
        };

        let (events, _completed) = result_to_events(&result);
        assert_eq!(events.len(), 1, "Single-signal group: only GroupCreated, no SignalAddedToGroup");
        match &events[0] {
            SystemEvent::GroupCreated { group_id: gid, seed_signal_id, .. } => {
                assert_eq!(*gid, group_id);
                assert_eq!(*seed_signal_id, Some(seed_id));
            }
            other => panic!("Expected GroupCreated, got {:?}", other),
        }
    }

    #[test]
    fn multi_signal_group_emits_group_created_plus_adds() {
        let group_id = Uuid::new_v4();
        let seed_id = Uuid::new_v4();
        let sig2 = Uuid::new_v4();
        let sig3 = Uuid::new_v4();

        let result = CoalescingResult {
            new_groups: vec![ProtoGroup {
                group_id,
                label: "Multi signal".into(),
                queries: vec!["q1".into()],
                signal_ids: vec![
                    (seed_id, 1.0),
                    (sig2, 0.85),
                    (sig3, 0.72),
                ],
            }],
            fed_signals: vec![],
            refined_queries: vec![],
        };

        let (events, _completed) = result_to_events(&result);
        assert_eq!(events.len(), 3, "1 GroupCreated + 2 SignalAddedToGroup");

        // First event: GroupCreated with seed
        match &events[0] {
            SystemEvent::GroupCreated { seed_signal_id, .. } => {
                assert_eq!(*seed_signal_id, Some(seed_id));
            }
            other => panic!("Expected GroupCreated, got {:?}", other),
        }

        // Second event: sig2 added
        match &events[1] {
            SystemEvent::SignalAddedToGroup { signal_id, confidence, .. } => {
                assert_eq!(*signal_id, sig2);
                assert!((confidence - 0.85).abs() < f64::EPSILON);
            }
            other => panic!("Expected SignalAddedToGroup, got {:?}", other),
        }

        // Third event: sig3 added
        match &events[2] {
            SystemEvent::SignalAddedToGroup { signal_id, confidence, .. } => {
                assert_eq!(*signal_id, sig3);
                assert!((confidence - 0.72).abs() < f64::EPSILON);
            }
            other => panic!("Expected SignalAddedToGroup, got {:?}", other),
        }
    }

    #[test]
    fn empty_signal_ids_produces_group_created_without_seed() {
        let group_id = Uuid::new_v4();
        let result = CoalescingResult {
            new_groups: vec![ProtoGroup {
                group_id,
                label: "Empty group".into(),
                queries: vec![],
                signal_ids: vec![],
            }],
            fed_signals: vec![],
            refined_queries: vec![],
        };

        let (events, _completed) = result_to_events(&result);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SystemEvent::GroupCreated { seed_signal_id, .. } => {
                assert_eq!(*seed_signal_id, None, "No signals means no seed");
            }
            other => panic!("Expected GroupCreated, got {:?}", other),
        }
    }

    #[test]
    fn fed_signals_emit_signal_added_to_group() {
        let group_id = Uuid::new_v4();
        let sig1 = Uuid::new_v4();
        let sig2 = Uuid::new_v4();

        let result = CoalescingResult {
            new_groups: vec![],
            fed_signals: vec![
                FedSignal { signal_id: sig1, group_id, confidence: 0.9 },
                FedSignal { signal_id: sig2, group_id, confidence: 0.6 },
            ],
            refined_queries: vec![],
        };

        let (events, _completed) = result_to_events(&result);
        assert_eq!(events.len(), 2);
        for event in &events {
            match event {
                SystemEvent::SignalAddedToGroup { group_id: gid, .. } => {
                    assert_eq!(*gid, group_id);
                }
                other => panic!("Expected SignalAddedToGroup, got {:?}", other),
            }
        }
    }

    #[test]
    fn refined_queries_emit_group_queries_refined() {
        let group_id = Uuid::new_v4();
        let result = CoalescingResult {
            new_groups: vec![],
            fed_signals: vec![],
            refined_queries: vec![
                (group_id, vec!["new query 1".into(), "new query 2".into()]),
            ],
        };

        let (events, _completed) = result_to_events(&result);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SystemEvent::GroupQueriesRefined { group_id: gid, queries } => {
                assert_eq!(*gid, group_id);
                assert_eq!(queries.len(), 2);
            }
            other => panic!("Expected GroupQueriesRefined, got {:?}", other),
        }
    }

    #[test]
    fn full_result_emits_all_event_types_in_order() {
        let group_id = Uuid::new_v4();
        let existing_group = Uuid::new_v4();
        let seed = Uuid::new_v4();
        let sig2 = Uuid::new_v4();
        let fed_sig = Uuid::new_v4();

        let result = CoalescingResult {
            new_groups: vec![ProtoGroup {
                group_id,
                label: "New group".into(),
                queries: vec!["q".into()],
                signal_ids: vec![(seed, 1.0), (sig2, 0.8)],
            }],
            fed_signals: vec![FedSignal {
                signal_id: fed_sig,
                group_id: existing_group,
                confidence: 0.75,
            }],
            refined_queries: vec![(existing_group, vec!["refined".into()])],
        };

        let (events, completed) = result_to_events(&result);
        // GroupCreated, SignalAddedToGroup (sig2), SignalAddedToGroup (fed), GroupQueriesRefined
        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], SystemEvent::GroupCreated { .. }));
        assert!(matches!(events[1], SystemEvent::SignalAddedToGroup { .. }));
        assert!(matches!(events[2], SystemEvent::SignalAddedToGroup { .. }));
        assert!(matches!(events[3], SystemEvent::GroupQueriesRefined { .. }));

        match completed {
            crate::domains::coalescing::events::CoalescingEvent::CoalescingCompleted {
                new_groups,
                fed_signals,
                refined_groups,
            } => {
                assert_eq!(new_groups, 1);
                assert_eq!(fed_signals, 1);
                assert_eq!(refined_groups, 1);
            }
            other => panic!("Expected CoalescingCompleted, got {:?}", other),
        }
    }

    // --- Adversarial: try to break the event mapping ---

    #[test]
    fn multiple_groups_interleave_events_correctly() {
        let g1 = Uuid::new_v4();
        let g2 = Uuid::new_v4();
        let s1a = Uuid::new_v4();
        let s1b = Uuid::new_v4();
        let s2a = Uuid::new_v4();
        let s2b = Uuid::new_v4();
        let s2c = Uuid::new_v4();

        let result = CoalescingResult {
            new_groups: vec![
                ProtoGroup {
                    group_id: g1,
                    label: "Group 1".into(),
                    queries: vec!["q1".into()],
                    signal_ids: vec![(s1a, 1.0), (s1b, 0.9)],
                },
                ProtoGroup {
                    group_id: g2,
                    label: "Group 2".into(),
                    queries: vec!["q2".into()],
                    signal_ids: vec![(s2a, 1.0), (s2b, 0.8), (s2c, 0.7)],
                },
            ],
            fed_signals: vec![],
            refined_queries: vec![],
        };

        let (events, _completed) = result_to_events(&result);
        // g1: GroupCreated + 1 add, g2: GroupCreated + 2 adds = 5 total
        assert_eq!(events.len(), 5);

        // Group 1 events come first (group ordering preserved)
        match &events[0] {
            SystemEvent::GroupCreated { group_id, seed_signal_id, .. } => {
                assert_eq!(*group_id, g1);
                assert_eq!(*seed_signal_id, Some(s1a));
            }
            other => panic!("Expected GroupCreated for g1, got {:?}", other),
        }
        match &events[1] {
            SystemEvent::SignalAddedToGroup { signal_id, group_id, .. } => {
                assert_eq!(*signal_id, s1b);
                assert_eq!(*group_id, g1);
            }
            other => panic!("Expected SignalAddedToGroup for g1, got {:?}", other),
        }

        // Group 2 events come second
        match &events[2] {
            SystemEvent::GroupCreated { group_id, seed_signal_id, .. } => {
                assert_eq!(*group_id, g2);
                assert_eq!(*seed_signal_id, Some(s2a));
            }
            other => panic!("Expected GroupCreated for g2, got {:?}", other),
        }
        // Two more adds for g2
        assert!(matches!(&events[3], SystemEvent::SignalAddedToGroup { signal_id, .. } if *signal_id == s2b));
        assert!(matches!(&events[4], SystemEvent::SignalAddedToGroup { signal_id, .. } if *signal_id == s2c));
    }

    #[test]
    fn same_signal_in_multiple_groups_emits_separate_events() {
        let g1 = Uuid::new_v4();
        let g2 = Uuid::new_v4();
        let shared_signal = Uuid::new_v4();
        let seed1 = Uuid::new_v4();
        let seed2 = Uuid::new_v4();

        let result = CoalescingResult {
            new_groups: vec![
                ProtoGroup {
                    group_id: g1,
                    label: "Group 1".into(),
                    queries: vec![],
                    signal_ids: vec![(seed1, 1.0), (shared_signal, 0.8)],
                },
                ProtoGroup {
                    group_id: g2,
                    label: "Group 2".into(),
                    queries: vec![],
                    signal_ids: vec![(seed2, 1.0), (shared_signal, 0.75)],
                },
            ],
            fed_signals: vec![],
            refined_queries: vec![],
        };

        let (events, _completed) = result_to_events(&result);
        // g1: 1 create + 1 add, g2: 1 create + 1 add = 4
        assert_eq!(events.len(), 4);

        // shared_signal appears twice with different group_ids and confidences
        let adds: Vec<_> = events.iter().filter_map(|e| match e {
            SystemEvent::SignalAddedToGroup { signal_id, group_id, confidence } if *signal_id == shared_signal => {
                Some((*group_id, *confidence))
            }
            _ => None,
        }).collect();

        assert_eq!(adds.len(), 2, "Same signal should appear in both groups");
        assert!(adds.iter().any(|(gid, c)| *gid == g1 && (*c - 0.8).abs() < f64::EPSILON));
        assert!(adds.iter().any(|(gid, c)| *gid == g2 && (*c - 0.75).abs() < f64::EPSILON));
    }

    #[test]
    fn negative_confidence_passes_through_unmolested() {
        let result = CoalescingResult {
            new_groups: vec![ProtoGroup {
                group_id: Uuid::new_v4(),
                label: "Test".into(),
                queries: vec![],
                signal_ids: vec![
                    (Uuid::new_v4(), 1.0),
                    (Uuid::new_v4(), -0.5),
                ],
            }],
            fed_signals: vec![],
            refined_queries: vec![],
        };

        let (events, _completed) = result_to_events(&result);
        match &events[1] {
            SystemEvent::SignalAddedToGroup { confidence, .. } => {
                assert!((*confidence - (-0.5)).abs() < f64::EPSILON,
                    "Mapping layer should not clamp — that's the coalescer's job");
            }
            other => panic!("Expected SignalAddedToGroup, got {:?}", other),
        }
    }

    #[test]
    fn hundred_queries_in_group_all_round_trip() {
        let queries: Vec<String> = (0..100).map(|i| format!("query {i}")).collect();
        let group_id = Uuid::new_v4();

        let result = CoalescingResult {
            new_groups: vec![ProtoGroup {
                group_id,
                label: "Large query set".into(),
                queries: queries.clone(),
                signal_ids: vec![(Uuid::new_v4(), 1.0)],
            }],
            fed_signals: vec![],
            refined_queries: vec![],
        };

        let (events, _completed) = result_to_events(&result);
        match &events[0] {
            SystemEvent::GroupCreated { queries: qs, .. } => {
                assert_eq!(qs.len(), 100);
                assert_eq!(qs[0], "query 0");
                assert_eq!(qs[99], "query 99");
            }
            other => panic!("Expected GroupCreated, got {:?}", other),
        }
    }

    #[test]
    fn many_fed_signals_to_different_groups() {
        let g1 = Uuid::new_v4();
        let g2 = Uuid::new_v4();

        let fed_signals: Vec<FedSignal> = (0..50).map(|i| FedSignal {
            signal_id: Uuid::new_v4(),
            group_id: if i % 2 == 0 { g1 } else { g2 },
            confidence: i as f64 / 50.0,
        }).collect();

        let result = CoalescingResult {
            new_groups: vec![],
            fed_signals,
            refined_queries: vec![],
        };

        let (events, _completed) = result_to_events(&result);
        assert_eq!(events.len(), 50);

        let g1_count = events.iter().filter(|e| matches!(e, SystemEvent::SignalAddedToGroup { group_id, .. } if *group_id == g1)).count();
        let g2_count = events.iter().filter(|e| matches!(e, SystemEvent::SignalAddedToGroup { group_id, .. } if *group_id == g2)).count();
        assert_eq!(g1_count, 25);
        assert_eq!(g2_count, 25);
    }
}

#[cfg(test)]
mod coalescer_tests {
    //! Tests for the Coalescer workflow — MOCK → FUNCTION → OUTPUT.
    //! Exercises seed mode and feed mode via MockGraphQueries + MockAgent.

    use std::sync::Arc;

    use uuid::Uuid;

    use rootsignal_graph::{GraphQueries, GroupBrief, SignalDetail, SignalSearchResult};

    use crate::domains::coalescing::activities::coalescer::Coalescer;
    use crate::testing::{FixedEmbedder, MockAgent, MockGraphQueries, TEST_EMBEDDING_DIM};

    #[tokio::test]
    async fn feed_mode_excludes_signals_already_in_group() {
        let existing_signal = Uuid::new_v4();
        let new_signal = Uuid::new_v4();
        let group_id = Uuid::new_v4();

        let graph = MockGraphQueries::new()
            .with_group_landscape(vec![GroupBrief {
                id: group_id,
                label: "Housing issues".into(),
                queries: vec!["rent increase".into()],
                signal_count: 1,
                member_ids: vec![existing_signal],
            }])
            .with_search_results(vec![
                SignalSearchResult {
                    id: existing_signal,
                    title: "Existing signal".into(),
                    summary: "Already in group".into(),
                    signal_type: "Concern".into(),
                    score: 0.9,
                },
                SignalSearchResult {
                    id: new_signal,
                    title: "New signal".into(),
                    summary: "Not yet in group".into(),
                    signal_type: "Concern".into(),
                    score: 0.8,
                },
            ])
            .with_signal_details(vec![
                SignalDetail {
                    id: existing_signal,
                    title: "Existing signal".into(),
                    summary: "Already in group".into(),
                    signal_type: "Concern".into(),
                    cause_heat: Some(0.5),
                },
                SignalDetail {
                    id: new_signal,
                    title: "New signal".into(),
                    summary: "Not yet in group".into(),
                    signal_type: "Concern".into(),
                    cause_heat: Some(0.7),
                },
            ]);

        // LLM returns both signals as adds — the coalescer should filter out the existing one
        let ai = MockAgent::with_response(serde_json::json!({
            "add": [
                { "signal_id": existing_signal.to_string(), "confidence": 0.9 },
                { "signal_id": new_signal.to_string(), "confidence": 0.85 }
            ],
            "refined_queries": []
        }));

        let coalescer = Coalescer::new(
            Arc::new(graph) as Arc<dyn GraphQueries>,
            Arc::new(ai) as Arc<dyn ai_client::Agent>,
            Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        );

        let result = coalescer.run(None).await.unwrap();

        assert_eq!(
            result.fed_signals.len(), 1,
            "Should only feed the new signal, not the existing member"
        );
        assert_eq!(result.fed_signals[0].signal_id, new_signal);
        assert_eq!(result.fed_signals[0].group_id, group_id);
    }

    #[tokio::test]
    async fn feed_mode_all_candidates_already_members_produces_no_fed_signals() {
        let sig_a = Uuid::new_v4();
        let sig_b = Uuid::new_v4();
        let group_id = Uuid::new_v4();

        let graph = MockGraphQueries::new()
            .with_group_landscape(vec![GroupBrief {
                id: group_id,
                label: "Transit".into(),
                queries: vec!["bus route".into()],
                signal_count: 2,
                member_ids: vec![sig_a, sig_b],
            }])
            .with_search_results(vec![
                SignalSearchResult {
                    id: sig_a,
                    title: "A".into(),
                    summary: "Already member".into(),
                    signal_type: "Concern".into(),
                    score: 0.9,
                },
                SignalSearchResult {
                    id: sig_b,
                    title: "B".into(),
                    summary: "Already member".into(),
                    signal_type: "Concern".into(),
                    score: 0.8,
                },
            ]);
        // No signal details needed — candidates should be filtered before LLM call

        let ai = MockAgent::with_response(serde_json::json!({
            "add": [],
            "refined_queries": []
        }));

        let coalescer = Coalescer::new(
            Arc::new(graph) as Arc<dyn GraphQueries>,
            Arc::new(ai) as Arc<dyn ai_client::Agent>,
            Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        );

        let result = coalescer.run(None).await.unwrap();
        assert!(
            result.fed_signals.is_empty(),
            "No new candidates means no fed signals"
        );
    }

    #[tokio::test]
    async fn seed_mode_skips_when_no_ungrouped_signals() {
        let graph = MockGraphQueries::new();
        let ai = MockAgent::with_response(serde_json::json!({
            "found_group": false,
            "skip_reason": "no signals"
        }));

        let coalescer = Coalescer::new(
            Arc::new(graph) as Arc<dyn GraphQueries>,
            Arc::new(ai) as Arc<dyn ai_client::Agent>,
            Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        );

        let result = coalescer.run(None).await.unwrap();
        assert!(result.new_groups.is_empty());
        assert!(result.fed_signals.is_empty());
        assert!(result.refined_queries.is_empty());
    }

    #[tokio::test]
    async fn feed_mode_refined_queries_propagate() {
        let group_id = Uuid::new_v4();
        let new_signal = Uuid::new_v4();

        let graph = MockGraphQueries::new()
            .with_group_landscape(vec![GroupBrief {
                id: group_id,
                label: "Safety".into(),
                queries: vec!["crime report".into()],
                signal_count: 3,
                member_ids: vec![],
            }])
            .with_search_results(vec![SignalSearchResult {
                id: new_signal,
                title: "New".into(),
                summary: "New signal".into(),
                signal_type: "Concern".into(),
                score: 0.9,
            }])
            .with_signal_details(vec![SignalDetail {
                id: new_signal,
                title: "New".into(),
                summary: "New signal".into(),
                signal_type: "Concern".into(),
                cause_heat: Some(0.6),
            }]);

        let ai = MockAgent::with_response(serde_json::json!({
            "add": [{ "signal_id": new_signal.to_string(), "confidence": 0.8 }],
            "refined_queries": ["updated crime report", "safety concern"]
        }));

        let coalescer = Coalescer::new(
            Arc::new(graph) as Arc<dyn GraphQueries>,
            Arc::new(ai) as Arc<dyn ai_client::Agent>,
            Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        );

        let result = coalescer.run(None).await.unwrap();
        assert_eq!(result.fed_signals.len(), 1);
        assert_eq!(result.refined_queries.len(), 1);
        assert_eq!(result.refined_queries[0].0, group_id);
        assert_eq!(result.refined_queries[0].1.len(), 2);
    }
}
