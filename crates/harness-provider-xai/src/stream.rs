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
    buffer: String,
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
            buffer: String::new(),
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
                return Some(Ok(Delta::Text(text)));
            }
        }

        match finish {
            Some(reason) => {
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
            None => None,
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
            // Check if we have a complete SSE line in the buffer
            if let Some(pos) = self.buffer.find('\n') {
                let line = self.buffer[..pos].trim_end_matches('\r').to_string();
                self.buffer = self.buffer[pos + 1..].to_string();

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

            // Need more bytes from the underlying stream
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => match std::str::from_utf8(&bytes) {
                    Ok(s) => self.buffer.push_str(s),
                    Err(e) => return Poll::Ready(Some(Err(ProviderError::Other(e.to_string())))),
                },
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(ProviderError::Other(e.to_string()))))
                }
                Poll::Ready(None) => {
                    // Flush any remaining tool calls if stream ended mid-tool
                    if !self.tool_call_builders.is_empty() {
                        let calls = self.flush_tool_calls();
                        for call in calls {
                            self.queue.push_back(Ok(Delta::ToolCall(call)));
                        }
                        if let Some(item) = self.queue.pop_front() {
                            return Poll::Ready(Some(item));
                        }
                    }
                    return Poll::Ready(None);
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
    async fn flushes_multiple_pending_tool_calls_on_stream_end() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"/tmp/a.txt\\\"}\"}},",
            "{\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"write_file\",\"arguments\":\"{\\\"path\\\":\\\"/tmp/b.txt\\\"}\"}}",
            "]},\"finish_reason\":null}]}\n"
        );

        let inner = stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from(payload))]);
        let mut sse = SseStream::new(inner);

        let mut emitted = Vec::new();
        while let Some(item) = sse.next().await {
            emitted.push(item.expect("valid stream item"));
        }

        assert_eq!(emitted.len(), 2);
        assert!(matches!(&emitted[0], Delta::ToolCall(call) if call.id == "call_a"));
        assert!(matches!(&emitted[1], Delta::ToolCall(call) if call.id == "call_b"));
    }
}
