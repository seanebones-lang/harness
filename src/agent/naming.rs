//! Session title suggestion.

use futures::StreamExt;
use harness_memory::Session;
use harness_provider_core::{ArcProvider, ChatRequest, Delta, Message, Role};

pub async fn suggest_session_name(provider: &ArcProvider, session: &Session) -> Option<String> {
    if session.name.is_some() {
        return None;
    }

    let first_user = session
        .messages
        .iter()
        .find(|m| matches!(m.role, Role::User))
        .map(|m| m.content.as_str().to_string())
        .unwrap_or_default();

    if first_user.is_empty() {
        return None;
    }

    let snippet = &first_user[..first_user.len().min(200)];
    let prompt = format!(
        "Summarise this task in 4 to 6 words. No punctuation, no quotes. \
         Reply with ONLY the title.\n\nTask: {snippet}"
    );

    let req = ChatRequest::new(&session.model).with_messages(vec![Message::user(&prompt)]);

    let Ok(mut stream) = provider.stream_chat(req).await else {
        return None;
    };
    let mut title = String::new();
    while let Some(Ok(Delta::Text(chunk))) = stream.next().await {
        title.push_str(&chunk);
    }

    let title = title.trim().to_string();
    if !title.is_empty() && title.len() < 80 {
        return Some(title);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use harness_provider_core::{DeltaStream, Pricing, Provider, ProviderError, StopReason};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    struct ScriptProvider {
        scripts: Mutex<Vec<Vec<Delta>>>,
        calls: AtomicUsize,
        fail: bool,
    }

    impl ScriptProvider {
        fn new(scripts: Vec<Vec<Delta>>) -> Self {
            Self {
                scripts: Mutex::new(scripts),
                calls: AtomicUsize::new(0),
                fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                scripts: Mutex::new(vec![]),
                calls: AtomicUsize::new(0),
                fail: true,
            }
        }
    }

    #[async_trait]
    impl Provider for ScriptProvider {
        fn name(&self) -> &str {
            "script"
        }

        fn model(&self) -> &str {
            "script-model"
        }

        async fn stream_chat(&self, _req: ChatRequest) -> Result<DeltaStream, ProviderError> {
            if self.fail {
                return Err(ProviderError::Other("boom".into()));
            }
            let idx = self.calls.fetch_add(1, Ordering::SeqCst);
            let batch = {
                let scripts = self.scripts.lock().unwrap_or_else(|e| e.into_inner());
                scripts.get(idx).cloned().unwrap_or_else(|| {
                    vec![Delta::Done {
                        stop_reason: StopReason::EndTurn,
                    }]
                })
            };
            Ok(Box::pin(stream::iter(batch.into_iter().map(Ok))))
        }

        fn pricing(&self) -> Option<Pricing> {
            None
        }
    }

    #[tokio::test]
    async fn suggest_session_name_skips_when_already_named() {
        let provider: ArcProvider = Arc::new(ScriptProvider::new(vec![]));
        let mut session = Session::new("script-model");
        session.name = Some("existing".into());
        session.push(Message::user("do a thing"));
        assert!(suggest_session_name(&provider, &session).await.is_none());
    }

    #[tokio::test]
    async fn suggest_session_name_skips_when_no_user_message() {
        let provider: ArcProvider = Arc::new(ScriptProvider::new(vec![]));
        let session = Session::new("script-model");
        assert!(suggest_session_name(&provider, &session).await.is_none());
    }

    #[tokio::test]
    async fn suggest_session_name_returns_trimmed_title() {
        let provider: ArcProvider = Arc::new(ScriptProvider::new(vec![vec![
            Delta::Text("  Fix auth token  ".into()),
            Delta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]]));
        let mut session = Session::new("script-model");
        session.push(Message::user("please fix the auth token helper"));
        let title = suggest_session_name(&provider, &session)
            .await
            .expect("title");
        assert_eq!(title, "Fix auth token");
    }

    #[tokio::test]
    async fn suggest_session_name_rejects_empty_or_overlong_titles() {
        let empty: ArcProvider = Arc::new(ScriptProvider::new(vec![vec![
            Delta::Text("   ".into()),
            Delta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]]));
        let mut session = Session::new("script-model");
        session.push(Message::user("task"));
        assert!(suggest_session_name(&empty, &session).await.is_none());

        let long_title = "x".repeat(80);
        let long: ArcProvider = Arc::new(ScriptProvider::new(vec![vec![
            Delta::Text(long_title),
            Delta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]]));
        assert!(suggest_session_name(&long, &session).await.is_none());
    }

    #[tokio::test]
    async fn suggest_session_name_returns_none_on_provider_error() {
        let provider: ArcProvider = Arc::new(ScriptProvider::failing());
        let mut session = Session::new("script-model");
        session.push(Message::user("task"));
        assert!(suggest_session_name(&provider, &session).await.is_none());
    }
}
