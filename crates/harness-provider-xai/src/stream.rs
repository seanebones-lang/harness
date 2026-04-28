//! SSE byte-stream → Delta stream adapter.

use futures::Stream;
use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use harness_provider_core::{Delta, ProviderError, StopReason, ToolCall, ToolCallFunction};

use crate::types::{PartialToolCall, StreamChunk};

/// Wraps a raw byte stream from reqwest and parses SSE into `Delta` items.
pub struct SseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    buffer: String,
    // assembles fragmented tool_call deltas keyed by index
    tool_call_builders: HashMap<usize, ToolCallBuilder>,
    done: bool,
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
            Some("tool_calls") => {
                // Emit all assembled tool calls, then Done
                let calls: Vec<ToolCall> = self.flush_tool_calls();
                if let Some(first) = calls.into_iter().next() {
                    return Some(Ok(Delta::ToolCall(first)));
                }
                Some(Ok(Delta::Done { stop_reason: StopReason::ToolUse }))
            }
            Some("stop") => Some(Ok(Delta::Done { stop_reason: StopReason::EndTurn })),
            Some("length") => Some(Ok(Delta::Done { stop_reason: StopReason::MaxTokens })),
            Some(other) => Some(Ok(Delta::Done { stop_reason: StopReason::Other(other.to_string()) })),
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
                Poll::Ready(Some(Ok(bytes))) => {
                    match std::str::from_utf8(&bytes) {
                        Ok(s) => self.buffer.push_str(s),
                        Err(e) => {
                            return Poll::Ready(Some(Err(ProviderError::Other(e.to_string()))))
                        }
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(ProviderError::Other(e.to_string()))))
                }
                Poll::Ready(None) => {
                    // Flush any remaining tool calls if stream ended mid-tool
                    if !self.tool_call_builders.is_empty() {
                        let calls = self.flush_tool_calls();
                        if let Some(call) = calls.into_iter().next() {
                            return Poll::Ready(Some(Ok(Delta::ToolCall(call))));
                        }
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
