//! Signal lint tool tests.
//!
//! Tests the lint tool implementations (pass, correct, reject) and
//! verdict state management. The LLM integration is not tested here —
//! that requires a live API key and is covered by integration tests.
//!
//! Run with: cargo test -p rootsignal-scout --test signal_lint_test

use std::sync::Arc;

use ai_client::tool::Tool;
use anyhow::Result;

use rootsignal_common::types::{ArchivedPage, ScoutScope};
use rootsignal_scout::pipeline::lint_tools::*;
use rootsignal_scout::pipeline::signal_lint::SignalLinter;
use rootsignal_scout::testing::MockFetcher;
use rootsignal_scout::testing::MockSignalStore;
use rootsignal_scout::infra::run_log::RunLogger;

fn test_scope() -> ScoutScope {
    ScoutScope {
        center_lat: 44.9537,
        center_lng: -93.0900,
        radius_km: 30.0,
        name: "test-region".to_string(),
    }
}

// ---------------------------------------------------------------------------
// PassSignalTool tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pass_signal_records_verdict() {
    let state = LintState::new();
    let tool = PassSignalTool { state: state.clone() };

    let result = tool.call(PassSignalArgs { node_id: "abc-123".into() }).await;

    assert!(result.is_ok());
    let verdicts = state.into_verdicts();
    assert!(matches!(verdicts.get("abc-123"), Some(LintVerdict::Pass)));
}

// ---------------------------------------------------------------------------
// CorrectSignalTool tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn correct_signal_records_correction() {
    let state = LintState::new();
    let tool = CorrectSignalTool { state: state.clone() };

    let result = tool
        .call(CorrectSignalArgs {
            node_id: "abc-123".into(),
            field: "title".into(),
            old_value: "Wrong Title".into(),
            new_value: "Correct Title".into(),
            reason: "Title didn't match source".into(),
        })
        .await;

    assert!(result.is_ok());
    let verdicts = state.into_verdicts();
    match verdicts.get("abc-123") {
        Some(LintVerdict::Correct { corrections }) => {
            assert_eq!(corrections.len(), 1);
            assert_eq!(corrections[0].field, "title");
            assert_eq!(corrections[0].new_value, "Correct Title");
        }
        other => panic!("Expected Correct verdict, got {:?}", other),
    }
}

#[tokio::test]
async fn multiple_corrections_accumulate() {
    let state = LintState::new();
    let tool = CorrectSignalTool { state: state.clone() };

    tool.call(CorrectSignalArgs {
        node_id: "abc-123".into(),
        field: "title".into(),
        old_value: "Old".into(),
        new_value: "New".into(),
        reason: "Fix".into(),
    })
    .await
    .unwrap();

    tool.call(CorrectSignalArgs {
        node_id: "abc-123".into(),
        field: "summary".into(),
        old_value: "Old summary".into(),
        new_value: "New summary".into(),
        reason: "Fix summary".into(),
    })
    .await
    .unwrap();

    let verdicts = state.into_verdicts();
    match verdicts.get("abc-123") {
        Some(LintVerdict::Correct { corrections }) => {
            assert_eq!(corrections.len(), 2);
        }
        other => panic!("Expected Correct with 2 corrections, got {:?}", other),
    }
}

#[tokio::test]
async fn immutable_field_correction_rejected() {
    let state = LintState::new();
    let tool = CorrectSignalTool { state: state.clone() };

    let result = tool
        .call(CorrectSignalArgs {
            node_id: "abc-123".into(),
            field: "signal_type".into(),
            old_value: "Gathering".into(),
            new_value: "Notice".into(),
            reason: "Wrong type".into(),
        })
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().0.contains("not correctable"));
}

// ---------------------------------------------------------------------------
// RejectSignalTool tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reject_signal_records_verdict() {
    let state = LintState::new();
    let tool = RejectSignalTool { state: state.clone() };

    let result = tool
        .call(RejectSignalArgs {
            node_id: "abc-123".into(),
            reason: "Hallucinated content".into(),
        })
        .await;

    assert!(result.is_ok());
    let verdicts = state.into_verdicts();
    match verdicts.get("abc-123") {
        Some(LintVerdict::Reject { reason }) => {
            assert_eq!(reason, "Hallucinated content");
        }
        other => panic!("Expected Reject verdict, got {:?}", other),
    }
}

#[tokio::test]
async fn rejected_signal_cannot_be_passed() {
    let state = LintState::new();
    let reject_tool = RejectSignalTool { state: state.clone() };
    let pass_tool = PassSignalTool { state: state.clone() };

    reject_tool
        .call(RejectSignalArgs {
            node_id: "abc-123".into(),
            reason: "Bad signal".into(),
        })
        .await
        .unwrap();

    let result = pass_tool.call(PassSignalArgs { node_id: "abc-123".into() }).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().0.contains("rejected"));
}

#[tokio::test]
async fn rejected_signal_cannot_be_corrected() {
    let state = LintState::new();
    let reject_tool = RejectSignalTool { state: state.clone() };
    let correct_tool = CorrectSignalTool { state: state.clone() };

    reject_tool
        .call(RejectSignalArgs {
            node_id: "abc-123".into(),
            reason: "Bad signal".into(),
        })
        .await
        .unwrap();

    let result = correct_tool
        .call(CorrectSignalArgs {
            node_id: "abc-123".into(),
            field: "title".into(),
            old_value: "Old".into(),
            new_value: "New".into(),
            reason: "Fix".into(),
        })
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().0.contains("rejected"));
}

// ---------------------------------------------------------------------------
// ReadSourceTool tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_source_returns_archived_content() {
    let fetcher = MockFetcher::new()
        .on_page("https://example.com/event", ArchivedPage {
            id: uuid::Uuid::new_v4(),
            source_id: uuid::Uuid::new_v4(),
            fetched_at: chrono::Utc::now(),
            content_hash: "abc".into(),
            raw_html: "<p>Event details</p>".into(),
            markdown: "# Community Event\n\nJoin us Saturday...".into(),
            title: Some("Community Event".into()),
            links: vec![],
            published_at: None,
        });

    let tool = ReadSourceTool { fetcher: Arc::new(fetcher) };

    let result = tool.call(ReadSourceArgs { url: "https://example.com/event".into() }).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Community Event"));
}

#[tokio::test]
async fn read_source_unregistered_url_returns_error() {
    let fetcher = MockFetcher::new();
    let tool = ReadSourceTool { fetcher: Arc::new(fetcher) };

    let result = tool.call(ReadSourceArgs { url: "https://unknown.com".into() }).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().0.contains("SOURCE_UNREADABLE"));
}

// ---------------------------------------------------------------------------
// SignalLinter — empty run produces no events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_run_produces_no_lint_events() -> Result<()> {
    let store: Arc<dyn rootsignal_scout::pipeline::traits::SignalStore> =
        Arc::new(MockSignalStore::new());
    let fetcher: Arc<dyn rootsignal_scout::pipeline::traits::ContentFetcher> =
        Arc::new(MockFetcher::new());
    let logger = RunLogger::noop();

    let linter = SignalLinter::new(
        store,
        fetcher,
        "fake-api-key".into(),
        test_scope(),
        logger,
    );

    let result = linter.run().await?;

    assert_eq!(result.passed, 0);
    assert_eq!(result.corrected, 0);
    assert_eq!(result.rejected, 0);

    Ok(())
}

// ---------------------------------------------------------------------------
// LintState — verdict aggregation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lint_state_collects_mixed_verdicts() {
    let state = LintState::new();

    let pass = PassSignalTool { state: state.clone() };
    let reject = RejectSignalTool { state: state.clone() };
    let correct = CorrectSignalTool { state: state.clone() };

    pass.call(PassSignalArgs { node_id: "sig-1".into() }).await.unwrap();
    reject
        .call(RejectSignalArgs { node_id: "sig-2".into(), reason: "Bad".into() })
        .await
        .unwrap();
    correct
        .call(CorrectSignalArgs {
            node_id: "sig-3".into(),
            field: "title".into(),
            old_value: "Old".into(),
            new_value: "New".into(),
            reason: "Fix".into(),
        })
        .await
        .unwrap();

    let verdicts = state.into_verdicts();
    assert_eq!(verdicts.len(), 3);
    assert!(matches!(verdicts.get("sig-1"), Some(LintVerdict::Pass)));
    assert!(matches!(verdicts.get("sig-2"), Some(LintVerdict::Reject { .. })));
    assert!(matches!(verdicts.get("sig-3"), Some(LintVerdict::Correct { .. })));
}
