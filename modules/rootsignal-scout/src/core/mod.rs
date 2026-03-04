// Core engine types: aggregate, events, stats, engine, projection.

pub mod aggregate;
pub mod embedding_cache;
pub mod engine;
pub mod events;
pub mod extractor;
pub mod pipeline_events;
pub mod postgres_store;
pub mod projection;
pub mod stats;

#[cfg(test)]
mod engine_tests {
    use std::sync::Arc;

    use super::aggregate::PipelineState;
    use super::engine::{build_engine, ScoutEngineDeps};
    use crate::domains::scrape::events::ScrapeEvent;

    #[tokio::test]
    async fn scrape_event_updates_aggregate_state() {
        let deps = ScoutEngineDeps::new(
            Arc::new(crate::testing::MockSignalReader::new()),
            Arc::new(crate::infra::embedder::NoOpEmbedder),
            "test",
        );
        let engine = build_engine(deps, None);

        let event = ScrapeEvent::ContentFetched {
            run_id: uuid::Uuid::new_v4(),
            url: "https://test.com".into(),
            canonical_key: "test".into(),
            content_hash: "abc123".into(),
            link_count: 0,
        };
        let result = engine.emit(event).settled().await;
        assert!(result.is_ok(), "settled should succeed: {:?}", result.err());

        let state = engine.singleton::<PipelineState>();
        assert_eq!(state.stats.urls_scraped, 1, "aggregator should have incremented urls_scraped");
    }
}
