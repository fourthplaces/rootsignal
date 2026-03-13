//! Server-Sent Events (SSE) streaming support for Claude chat completions.

use anyhow::Result;
use futures::{Stream, StreamExt};
use serde::Deserialize;

/// Internal streaming event from the Claude API.
#[derive(Debug, Clone)]
pub(crate) enum ClaudeStreamEvent {
    /// A text delta chunk.
    Delta(String),
    /// Stream complete.
    Done,
}

#[derive(Debug, Deserialize)]
struct ContentBlockDelta {
    delta: DeltaPayload,
}

#[derive(Debug, Deserialize)]
struct DeltaPayload {
    #[serde(default)]
    text: Option<String>,
}

/// Parse an SSE stream from the Claude Messages API.
///
/// Claude SSE format uses `event:` field lines:
/// - `content_block_delta` with `{"delta":{"type":"text_delta","text":"..."}}`
/// - `message_stop` signals completion
pub(crate) fn parse_claude_sse_stream(
    response: reqwest::Response,
) -> impl Stream<Item = Result<ClaudeStreamEvent>> {
    async_stream::try_stream! {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE events (separated by \n\n)
            while let Some(event_end) = buffer.find("\n\n") {
                let event_block = buffer[..event_end].to_string();
                buffer = buffer[event_end + 2..].to_string();

                let mut event_type = None;
                let mut data_str = None;

                for line in event_block.lines() {
                    if let Some(et) = line.strip_prefix("event: ") {
                        event_type = Some(et.to_string());
                    } else if let Some(d) = line.strip_prefix("data: ") {
                        data_str = Some(d.to_string());
                    }
                }

                match event_type.as_deref() {
                    Some("content_block_delta") => {
                        if let Some(ref data) = data_str {
                            if let Ok(block) = serde_json::from_str::<ContentBlockDelta>(data) {
                                if let Some(text) = block.delta.text {
                                    if !text.is_empty() {
                                        yield ClaudeStreamEvent::Delta(text);
                                    }
                                }
                            }
                        }
                    }
                    Some("message_stop") => {
                        yield ClaudeStreamEvent::Done;
                        return;
                    }
                    _ => {
                        // message_start, content_block_start, content_block_stop, ping — skip
                    }
                }
            }
        }
    }
}
