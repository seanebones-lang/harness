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
