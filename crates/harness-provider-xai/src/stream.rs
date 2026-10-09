//! SSE byte-stream → Delta stream adapter.

use futures::Stream;
use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use harness_provider_core::{Delta, ProviderError, StopReason, ToolCall, ToolCallFunction};

use crate::types::{PartialToolCall, StreamChunk, UsageInfo};

/// Wraps a raw byte stream from reqwest and parses SSE into `Delta` items.
pub struct SseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    buffer: harness_provider_core::LineBuffer,
    terminal_seen: bool,
    // assembles fragmented tool_call deltas keyed by index
    tool_call_builders: HashMap<usize, ToolCallBuilder>,
    done: bool,
    pending_usage: Option<UsageInfo>,
    // ready-to-emit items queued before the main poll loop re-runs
    queue: VecDeque<Result<Delta, ProviderError>>,
}

#[derive(Default)]
struct ToolCallBuilder {
    id: String,
    name: String,
    arguments: String,
}

impl SseStream {
    pub fn new(inner: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(inner),
            buffer: Default::default(),
            terminal_seen: false,
            tool_call_builders: HashMap::new(),
            done: false,
            pending_usage: None,
            queue: VecDeque::new(),
        }
    }

    fn parse_event(&mut self, line: &str) -> Option<Result<Delta, ProviderError>> {
        let data = line.strip_prefix("data: ")?;
        if data == "[DONE]" {
            self.done = true;
            if !self.terminal_seen {
                return Some(if self.tool_call_builders.is_empty() {
                    Ok(Delta::Done {
                        stop_reason: StopReason::EndTurn,
                    })
                } else {
                    Err(ProviderError::StreamEnded)
                });
            }
            return None;
        }

        let chunk: StreamChunk = match serde_json::from_str(data) {
            Ok(c) => c,
            Err(e) => return Some(Err(ProviderError::Json(e))),
        };

        // Usage arrives in the last chunk (stream_options: include_usage).
        if let Some(usage) = chunk.usage {
            if chunk.choices.is_empty() {
                return Some(Ok(Delta::Usage {
                    input_tokens: usage.prompt_tokens,
                    output_tokens: usage.completion_tokens,
                }));
            }
            // Store for emission after the choice is processed.
            self.pending_usage = Some(usage);
        }

        let choice = chunk.choices.into_iter().next()?;
        let finish = choice.finish_reason.as_deref();
        let delta = choice.delta;

        // Accumulate tool call fragments
        if let Some(partials) = delta.tool_calls {
            for p in partials {
                self.apply_partial(p);
            }
        }

        if let Some(text) = delta.content {
            if !text.is_empty() {
                self.queue.push_back(Ok(Delta::Text(text)));
            }
        }

        match finish {
            Some(reason) => {
                self.terminal_seen = true;
                let stop_reason = match reason {
                    "tool_calls" => StopReason::ToolUse,
                    "stop" => StopReason::EndTurn,
                    "length" => StopReason::MaxTokens,
                    other => StopReason::Other(other.to_string()),
                };

                // For tool_calls, queue all assembled calls first.
                if matches!(stop_reason, StopReason::ToolUse) {
                    let calls = self.flush_tool_calls();
                    for call in calls {
                        self.queue.push_back(Ok(Delta::ToolCall(call)));
                    }
                }

                // Emit usage before Done if available.
                if let Some(u) = self.pending_usage.take() {
                    self.queue.push_back(Ok(Delta::Usage {
                        input_tokens: u.prompt_tokens,
                        output_tokens: u.completion_tokens,
                    }));
                }

                self.queue.push_back(Ok(Delta::Done { stop_reason }));
                self.queue.pop_front()
            }
            None => self.queue.pop_front(),
        }
    }

    fn apply_partial(&mut self, p: PartialToolCall) {
        let builder = self.tool_call_builders.entry(p.index).or_default();
        if let Some(id) = p.id {
            builder.id = id;
        }
        if let Some(f) = p.function {
            if let Some(name) = f.name {
                builder.name = name;
            }
            if let Some(args) = f.arguments {
                builder.arguments.push_str(&args);
            }
        }
    }

    fn flush_tool_calls(&mut self) -> Vec<ToolCall> {
        let mut calls: Vec<(usize, ToolCall)> = self
            .tool_call_builders
            .drain()
            .map(|(idx, b)| {
                (
                    idx,
                    ToolCall {
                        id: b.id,
                        kind: "function".into(),
                        function: ToolCallFunction {
                            name: b.name,
                            arguments: b.arguments,
                        },
                    },
                )
            })
            .collect();
        calls.sort_by_key(|(idx, _)| *idx);
        calls.into_iter().map(|(_, c)| c).collect()
    }
}

impl Stream for SseStream {
    type Item = Result<Delta, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Drain pre-queued items (tool calls + usage + done).
        if let Some(item) = self.queue.pop_front() {
            return Poll::Ready(Some(item));
        }

        if self.done {
            return Poll::Ready(None);
        }

        loop {
            match self.buffer.next_line() {
                Ok(Some(line)) => {
                    if line.starts_with("data: ") {
                        if let Some(result) = self.parse_event(&line) {
                            return Poll::Ready(Some(result));
                        }
                        if self.done {
                            return Poll::Ready(None);
                        }
                    }
                    continue;
                }
                Err(error) => {
                    self.done = true;
                    return Poll::Ready(Some(Err(error)));
                }
                Ok(None) => {}
            }

            // Need more bytes from the underlying stream
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => self.buffer.push(&bytes),
                Poll::Ready(Some(Err(error))) => {
                    self.done = true;
                    return Poll::Ready(Some(Err(ProviderError::Other(error.to_string()))));
                }
                Poll::Ready(None) => {
                    self.done = true;
                    return if self.terminal_seen {
                        Poll::Ready(None)
                    } else {
                        Poll::Ready(Some(Err(ProviderError::StreamEnded)))
                    };
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SseStream;
    use bytes::Bytes;
    use futures::{stream, StreamExt};
    use harness_provider_core::{Delta, StopReason};

    #[tokio::test]
    async fn emits_all_tool_calls_when_finish_reason_is_tool_calls() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}},",
            "{\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"write_file\",\"arguments\":\"{\\\"path\\\":\"}}",
            "]},\"finish_reason\":null}]}\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"function\":{\"arguments\":\"\\\"/tmp/a.txt\\\"}\"}},",
            "{\"index\":1,\"function\":{\"arguments\":\"\\\"/tmp/b.txt\\\"}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":3,\"total_tokens\":13}}\n",
            "data: [DONE]\n"
        );

        let inner = stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from(payload))]);
        let mut sse = SseStream::new(inner);

        let mut emitted = Vec::new();
        while let Some(item) = sse.next().await {
            emitted.push(item.expect("valid stream item"));
        }

        assert!(matches!(&emitted[0], Delta::ToolCall(call) if call.id == "call_a"));
        assert!(matches!(&emitted[1], Delta::ToolCall(call) if call.id == "call_b"));
        assert!(matches!(
            &emitted[2],
            Delta::Usage {
                input_tokens: 10,
                output_tokens: 3
            }
        ));
        assert!(matches!(
            &emitted[3],
            Delta::Done {
                stop_reason: StopReason::ToolUse
            }
        ));
    }

    #[tokio::test]
    async fn rejects_pending_tool_calls_on_incomplete_stream() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"/tmp/a.txt\\\"}\"}},",
            "{\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"write_file\",\"arguments\":\"{\\\"path\\\":\\\"/tmp/b.txt\\\"}\"}}",
            "]},\"finish_reason\":null}]}\n"
        );

        let inner = stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from(payload))]);
        let mut sse = SseStream::new(inner);

        assert!(matches!(
            sse.next().await,
            Some(Err(harness_provider_core::ProviderError::StreamEnded))
        ));
        assert!(sse.next().await.is_none());
    }

    mod sse_proptest {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn malformed_json_line_does_not_panic(garbage in prop::collection::vec(any::<u8>(), 0..40)) {
                let line = format!("data: {}\n", String::from_utf8_lossy(&garbage));
                let inner = stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from(line))]);
                let rt = tokio::runtime::Runtime::new().expect("runtime");
                rt.block_on(async {
                    let mut sse = SseStream::new(inner);
                    while let Some(item) = sse.next().await {
                        let _ = item;
                    }
                });
            }
        }
    }
    #[tokio::test]
    async fn combined_final_chunk_preserves_unicode_usage_and_completion() {
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"hé中🦀\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n";
        for split in 0..=payload.len() {
            let chunks = vec![
                Ok(Bytes::copy_from_slice(&payload.as_bytes()[..split])),
                Ok(Bytes::copy_from_slice(&payload.as_bytes()[split..])),
            ];
            let mut parsed = SseStream::new(stream::iter(chunks));
            assert!(matches!(parsed.next().await, Some(Ok(Delta::Text(text))) if text == "hé中🦀"));
            assert!(matches!(
                parsed.next().await,
                Some(Ok(Delta::Usage {
                    input_tokens: 3,
                    output_tokens: 2
                }))
            ));
            assert!(matches!(
                parsed.next().await,
                Some(Ok(Delta::Done {
                    stop_reason: StopReason::EndTurn
                }))
            ));
            assert!(parsed.next().await.is_none());
        }
    }
}
