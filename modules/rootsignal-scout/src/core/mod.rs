pub mod aggregate;
pub mod embedding_cache;
pub mod engine;
pub mod events;
pub mod extractor;
pub mod pipeline_events;
pub mod postgres_store;
pub mod projection;
pub mod run_scope;
pub mod stats;

#[cfg(test)]
mod engine_tests {
    use std::sync::Arc;

    use super::aggregate::PipelineState;
    use super::engine::{build_engine, ScoutEngineDeps};
    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};
    use crate::testing::TestScrapeRoleCompleted;

    #[tokio::test]
    async fn scrape_event_updates_aggregate_state() {
        let deps = ScoutEngineDeps::new(
            Arc::new(crate::testing::MockSignalReader::new()),
            Arc::new(crate::infra::embedder::NoOpEmbedder),
            "test",
        );
        let engine = build_engine(deps, None);

        let event = TestScrapeRoleCompleted::builder()
            .role(ScrapeRole::TensionWeb)
            .urls_scraped(1)
            .build();
        let result = engine.emit(ScrapeEvent::from(event)).settled().await;
        assert!(result.is_ok(), "settled should succeed: {:?}", result.err());

        let state = engine.singleton::<PipelineState>();
        assert_eq!(state.stats.urls_scraped, 1, "aggregator should have incremented urls_scraped");
    }
}
