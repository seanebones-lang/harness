use chrono::{DateTime, Utc};
use harness_provider_core::Message;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SessionId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<Message>,
    pub model: String,
    pub system_prompt: Option<String>,
}

impl Session {
    pub fn new(model: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: None,
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
            model: model.into(),
            system_prompt: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn push(&mut self, msg: Message) {
        self.messages.push(msg);
        self.updated_at = Utc::now();
    }

    pub fn short_id(&self) -> &str {
        self.id.get(..8).unwrap_or(&self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_provider_core::Message;

    #[test]
    fn session_new_with_name_push_and_short_id() {
        let mut s = Session::new("claude-sonnet-4-6").with_name("demo");
        assert_eq!(s.name.as_deref(), Some("demo"));
        assert_eq!(s.model, "claude-sonnet-4-6");
        assert!(s.messages.is_empty());
        assert_eq!(s.short_id().len(), 8);
        assert!(s.id.starts_with(s.short_id()));

        let before = s.updated_at;
        s.push(Message::user("hi"));
        assert_eq!(s.messages.len(), 1);
        assert!(s.updated_at >= before);
    }
}
