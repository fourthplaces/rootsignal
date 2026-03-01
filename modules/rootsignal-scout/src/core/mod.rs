// Core engine types: aggregate, events, stats, engine, projection.

pub mod aggregate;
pub(crate) mod embedding_cache;
pub mod engine;
pub mod events;
pub mod extractor;
pub mod projection;
pub mod scrape_pipeline;
pub mod stats;

#[cfg(test)]
mod engine_tests {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use super::aggregate::PipelineState;
    use super::engine::{build_engine, ScoutEngineDeps};
    use super::events::{PipelineEvent, ScoutEvent};

    #[tokio::test]
    async fn seesaw_engine_applies_state_via_state_updater() {
        let state = Arc::new(RwLock::new(PipelineState::default()));
        let deps = ScoutEngineDeps {
            store: Arc::new(crate::testing::MockSignalReader::new()),
            embedder: Arc::new(crate::infra::embedder::NoOpEmbedder),
            region: None,
            fetcher: None,
            anthropic_api_key: None,
            graph_client: None,
            extractor: None,
            state: state.clone(),
            graph_projector: None,
            event_store: None,
            run_id: "test".into(),
            captured_events: None,
            budget: None,
            cancelled: None,
            pg_pool: None,
            archive: None,
        };
        let engine = build_engine(deps);

        let event = ScoutEvent::Pipeline(PipelineEvent::ContentFetched {
            url: "https://test.com".into(),
            canonical_key: "test".into(),
            content_hash: "abc123".into(),
            link_count: 0,
        });
        // dispatch().settled() drives the full settlement loop
        let result = engine.dispatch(event).settled().await;
        assert!(result.is_ok(), "settled should succeed: {:?}", result.err());

        let s = state.read().await;
        assert_eq!(s.stats.urls_scraped, 1, "state_updater should have incremented urls_scraped");
    }
}
