// Core engine types: aggregate, events, stats, engine, projection.

pub mod aggregate;
pub(crate) mod embedding_cache;
pub mod engine;
pub mod events;
pub mod extractor;
pub mod projection;
pub mod stats;

#[cfg(test)]
mod engine_tests {
    use std::sync::Arc;

    use super::engine::{build_engine, ScoutEngineDeps};
    use crate::domains::scrape::events::ScrapeEvent;

    #[tokio::test]
    async fn seesaw_engine_applies_state_via_apply_to_aggregate() {
        let deps = ScoutEngineDeps::new(
            Arc::new(crate::testing::MockSignalReader::new()),
            Arc::new(crate::infra::embedder::NoOpEmbedder),
            "test",
        );
        let state = deps.state.clone();
        let engine = build_engine(deps);

        let event = ScrapeEvent::ContentFetched {
            run_id: uuid::Uuid::new_v4(),
            url: "https://test.com".into(),
            canonical_key: "test".into(),
            content_hash: "abc123".into(),
            link_count: 0,
        };
        // dispatch().settled() drives the full settlement loop
        let result = engine.dispatch(event).settled().await;
        assert!(result.is_ok(), "settled should succeed: {:?}", result.err());

        let s = state.read().await;
        assert_eq!(s.stats.urls_scraped, 1, "apply_to_aggregate should have incremented urls_scraped");
    }
}
