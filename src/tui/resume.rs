//! Load session messages into TUI chat state (parity with export formatting).

use harness_memory::Session;
use harness_provider_core::Role;
use std::time::Instant;

use super::state::ChatMessage;

/// Populate chat messages from a resumed session.
pub fn load_session_into_chat(session: &Session, chat: &mut Vec<ChatMessage>) {
    chat.clear();
    for msg in &session.messages {
        match msg.role {
            Role::System => {
                let text = message_text(msg);
                if !text.is_empty() {
                    chat.push(ChatMessage {
                        role: "system".to_string(),
                        content: format!("[system] {text}"),
                        ts: Instant::now(),
                    });
                }
            }
            Role::User => {
                chat.push(ChatMessage {
                    role: "user".to_string(),
                    content: message_text(msg),
                    ts: Instant::now(),
                });
            }
            Role::Assistant => {
                let content = message_text(msg);
                if content.starts_with("__tool_calls__:") {
                    if let Some(json) = content.strip_prefix("__tool_calls__:") {
                        if let Ok(calls) = serde_json::from_str::<serde_json::Value>(json) {
                            if let Some(arr) = calls.as_array() {
                                for call in arr {
                                    let name =
                                        call["function"]["name"].as_str().unwrap_or("?");
                                    let args = call["function"]["arguments"]
                                        .as_str()
                                        .unwrap_or("{}");
                                    chat.push(ChatMessage {
                                        role: "assistant".to_string(),
                                        content: format!("→ `{name}`\n{args}"),
                                        ts: Instant::now(),
                                    });
                                }
                                continue;
                            }
                        }
                    }
                }
                if !content.is_empty() {
                    chat.push(ChatMessage {
                        role: "assistant".to_string(),
                        content,
                        ts: Instant::now(),
                    });
                }
            }
            Role::Tool => {
                let result = message_text(msg);
                let preview = if result.len() > 500 {
                    format!("{}\n… ({} bytes)", &result[..500], result.len())
                } else {
                    result
                };
                chat.push(ChatMessage {
                    role: "tool".to_string(),
                    content: format!("← {preview}"),
                    ts: Instant::now(),
                });
            }
        }
    }
}

fn message_text(msg: &harness_provider_core::Message) -> String {
    msg.content.as_str().to_string()
}

pub fn count_user_turns(session: &Session) -> usize {
    session
        .messages
        .iter()
        .filter(|m| m.role == Role::User)
        .count()
}

/// Count user turns from rendered TUI chat (excludes system/tool rows).
pub fn count_user_turns_from_chat(chat: &[ChatMessage]) -> usize {
    chat.iter().filter(|m| m.role == "user").count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_memory::Session;
    use harness_provider_core::{Message, MessageContent, Role};

    #[test]
    fn resume_formats_tool_calls() {
        let mut session = Session::new("test-model");
        session.messages.push(Message {
            role: Role::Assistant,
            content: MessageContent::Text(
                "__tool_calls__:[{\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"a.rs\\\"}\"}}]"
                    .into(),
            ),
            tool_call_id: None,
        });
        let mut chat = Vec::new();
        load_session_into_chat(&session, &mut chat);
        assert_eq!(chat.len(), 1);
        assert!(chat[0].content.contains("read_file"));
    }

    #[test]
    fn count_user_turns_excludes_assistant() {
        let mut session = Session::new("m");
        session.messages.push(Message::user("hi"));
        session.messages.push(Message::assistant("hello"));
        session.messages.push(Message::user("again"));
        assert_eq!(count_user_turns(&session), 2);
    }
}
