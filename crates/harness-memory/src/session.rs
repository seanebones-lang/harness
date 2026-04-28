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
        &self.id[..8]
    }
}
