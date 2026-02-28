// Core engine types: aggregate, events, deps, stats, engine, projection.

pub mod aggregate;
pub mod deps;
pub mod engine;
pub mod events;
pub mod projection;
pub mod stats;

#[cfg(test)]
mod engine_tests {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use super::aggregate::PipelineState;
    use super::engine::{build_seesaw_engine, ScoutEngineDeps};
    use super::events::{PipelineEvent, ScoutEvent};

    #[tokio::test]
    async fn seesaw_engine_applies_state_via_state_updater() {
        let state = Arc::new(RwLock::new(PipelineState::default()));
        let deps = ScoutEngineDeps {
            pipeline_deps: Arc::new(RwLock::new(None)),
            state: state.clone(),
            graph_projector: None,
            event_store: None,
            run_id: "test".into(),
            captured_events: None,
        };
        let engine = build_seesaw_engine(deps);

        let event = ScoutEvent::Pipeline(PipelineEvent::ContentFetched {
            url: "https://test.com".into(),
            canonical_key: "test".into(),
            content_hash: "abc123".into(),
            link_count: 0,
        });
        // process().settled() drives the full settlement loop (dispatch is fire-and-forget)
        let result = engine.process(event).settled().await;
        assert!(result.is_ok(), "settled should succeed: {:?}", result.err());

        let s = state.read().await;
        assert_eq!(s.stats.urls_scraped, 1, "state_updater should have incremented urls_scraped");
    }
}
