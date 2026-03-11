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
    async fn search_signals_tool_definition_has_required_query() {
        let graph = Arc::new(MockGraphQueries::new()) as Arc<dyn GraphQueries>;
        let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

        let tool = SearchSignalsTool { graph, embedder };
        let def = tool.definition().await;

        assert_eq!(def.name, "search_signals");
        let required = def.parameters["required"].as_array().unwrap();
        assert!(
            required.iter().any(|v| v.as_str() == Some("query")),
            "query should be required"
        );
    }

    #[tokio::test]
    async fn find_similar_tool_definition_has_required_signal_id() {
        let graph = Arc::new(MockGraphQueries::new()) as Arc<dyn GraphQueries>;
        let tool = FindSimilarTool { graph };
        let def = tool.definition().await;

        assert_eq!(def.name, "find_similar");
        let required = def.parameters["required"].as_array().unwrap();
        assert!(
            required.iter().any(|v| v.as_str() == Some("signal_id")),
            "signal_id should be required"
        );
    }
}

#[cfg(test)]
mod handler_event_mapping_tests {
    //! Verify the handler correctly maps CoalescingResult → SystemEvents.
    //! These exercise the event construction logic in the handler without
    //! going through the full engine.

    use uuid::Uuid;

    use rootsignal_common::events::SystemEvent;

    use crate::domains::coalescing::activities::types::{CoalescingResult, FedSignal, ProtoGroup};

    /// Simulate what the handler does: convert a CoalescingResult into SystemEvents.
    /// Extracted from mod.rs handler logic for testability.
    fn result_to_system_events(result: &CoalescingResult) -> Vec<SystemEvent> {
        let mut events = vec![];

        for group in &result.new_groups {
            events.push(SystemEvent::GroupCreated {
                group_id: group.group_id,
                label: group.label.clone(),
                queries: group.queries.clone(),
                seed_signal_id: group.signal_ids.first().map(|(id, _)| *id),
            });

            // Skip first signal (it's the seed, already in GroupCreated)
            for (signal_id, confidence) in group.signal_ids.iter().skip(1) {
                events.push(SystemEvent::SignalAddedToGroup {
                    signal_id: *signal_id,
                    group_id: group.group_id,
                    confidence: *confidence,
                });
            }
        }

        for fed in &result.fed_signals {
            events.push(SystemEvent::SignalAddedToGroup {
                signal_id: fed.signal_id,
                group_id: fed.group_id,
                confidence: fed.confidence,
            });
        }

        for (group_id, queries) in &result.refined_queries {
            events.push(SystemEvent::GroupQueriesRefined {
                group_id: *group_id,
                queries: queries.clone(),
            });
        }

        events
    }

    #[test]
    fn empty_result_produces_no_system_events() {
        let result = CoalescingResult {
            new_groups: vec![],
            fed_signals: vec![],
            refined_queries: vec![],
        };
        let events = result_to_system_events(&result);
        assert!(events.is_empty());
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

        let events = result_to_system_events(&result);
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

        let events = result_to_system_events(&result);
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

        let events = result_to_system_events(&result);
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

        let events = result_to_system_events(&result);
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

        let events = result_to_system_events(&result);
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

        let events = result_to_system_events(&result);
        // GroupCreated, SignalAddedToGroup (sig2), SignalAddedToGroup (fed), GroupQueriesRefined
        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], SystemEvent::GroupCreated { .. }));
        assert!(matches!(events[1], SystemEvent::SignalAddedToGroup { .. }));
        assert!(matches!(events[2], SystemEvent::SignalAddedToGroup { .. }));
        assert!(matches!(events[3], SystemEvent::GroupQueriesRefined { .. }));
    }
}
