//! AWS Bedrock Runtime provider (Converse API).
//!
//! Credentials: standard AWS env (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
//! optional `AWS_SESSION_TOKEN`, `AWS_REGION` / `AWS_DEFAULT_REGION`).
//! Model: `BEDROCK_MODEL_ID` or config `model`.
//!
//! HTTP Converse is signed with AWS SigV4. Unit tests cover config without network.

#![deny(missing_docs)]

use async_trait::async_trait;
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use futures::stream;
use harness_provider_core::{
    ArcProvider, ChatRequest, Delta, DeltaStream, MessageContent, Pricing, Provider, ProviderError,
    Role, StopReason,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::warn;

/// Default region when unset.
pub const DEFAULT_REGION: &str = "us-east-1";
/// Example model id (override in config / `BEDROCK_MODEL_ID`).
pub const DEFAULT_MODEL: &str = "anthropic.claude-3-5-sonnet-20241022-v2:0";

/// Bedrock provider configuration.
#[derive(Debug, Clone)]
pub struct BedrockConfig {
    /// Model id (foundation model or inference profile).
    pub model: String,
    /// AWS region.
    pub region: String,
    /// Access key id.
    pub access_key_id: String,
    /// Secret access key.
    pub secret_access_key: String,
    /// Optional session token.
    pub session_token: Option<String>,
}

impl BedrockConfig {
    /// Build from explicit fields.
    pub fn new(
        model: impl Into<String>,
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            region: region.into(),
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
        }
    }

    /// Load from environment + optional model override.
    pub fn from_env(model: Option<String>) -> anyhow::Result<Self> {
        let access_key_id = std::env::var("AWS_ACCESS_KEY_ID")
            .map_err(|_| anyhow::anyhow!("AWS_ACCESS_KEY_ID required for bedrock"))?;
        let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY")
            .map_err(|_| anyhow::anyhow!("AWS_SECRET_ACCESS_KEY required for bedrock"))?;
        let region = std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .unwrap_or_else(|_| DEFAULT_REGION.into());
        let model = model
            .or_else(|| std::env::var("BEDROCK_MODEL_ID").ok())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.into());
        let session_token = std::env::var("AWS_SESSION_TOKEN")
            .ok()
            .filter(|s| !s.is_empty());
        Ok(Self {
            model,
            region,
            access_key_id,
            secret_access_key,
            session_token,
        })
    }

    /// Validate non-empty credentials and model.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.access_key_id.is_empty() || self.secret_access_key.is_empty() {
            anyhow::bail!("bedrock requires AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY");
        }
        if self.model.is_empty() {
            anyhow::bail!("bedrock requires model / BEDROCK_MODEL_ID");
        }
        if self.region.is_empty() {
            anyhow::bail!("bedrock requires AWS_REGION");
        }
        Ok(())
    }
}

/// AWS Bedrock Runtime provider.
pub struct BedrockProvider {
    config: BedrockConfig,
    client: Client,
}

impl BedrockProvider {
    /// Construct a provider (validates config).
    pub fn new(config: BedrockConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self { config, client })
    }
}

/// Build `ArcProvider` from optional model / region overrides (keys from env).
pub fn build_arc(model: Option<String>, region: Option<String>) -> anyhow::Result<ArcProvider> {
    let mut cfg = BedrockConfig::from_env(model)?;
    if let Some(r) = region.filter(|s| !s.is_empty()) {
        cfg.region = r;
    }
    Ok(Arc::new(BedrockProvider::new(cfg)?))
}

fn content_text(c: &MessageContent) -> String {
    c.as_str().to_string()
}

fn messages_to_bedrock(req: &ChatRequest) -> (Option<String>, Vec<Value>) {
    let mut sys = req.system.clone();
    let mut out = Vec::new();
    for m in &req.messages {
        match m.role {
            Role::System => {
                let t = content_text(&m.content);
                if t.is_empty() {
                    continue;
                }
                sys = Some(match sys {
                    Some(s) => format!("{s}\n{t}"),
                    None => t,
                });
            }
            Role::User | Role::Tool => {
                let t = content_text(&m.content);
                if t.is_empty() {
                    continue;
                }
                out.push(json!({
                    "role": "user",
                    "content": [{"text": t}]
                }));
            }
            Role::Assistant => {
                let t = content_text(&m.content);
                if t.is_empty() {
                    continue;
                }
                out.push(json!({
                    "role": "assistant",
                    "content": [{"text": t}]
                }));
            }
        }
    }
    (sys, out)
}

fn signed_request(
    method: &str,
    url: &str,
    region: &str,
    access_key: &str,
    secret: &str,
    session: Option<&str>,
    body: &[u8],
) -> Result<http::Request<Vec<u8>>, ProviderError> {
    let mut settings = SigningSettings::default();
    settings.payload_checksum_kind = aws_sigv4::http_request::PayloadChecksumKind::XAmzSha256;

    let creds = Credentials::new(
        access_key,
        secret,
        session.map(|s| s.to_string()),
        None,
        "harness-bedrock",
    );
    let identity = creds.into();
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("bedrock")
        .time(SystemTime::now())
        .settings(settings)
        .build()
        .map_err(|e| ProviderError::Other(e.to_string()))?
        .into();

    let host = http::Uri::try_from(url)
        .map_err(|e| ProviderError::Other(e.to_string()))?
        .host()
        .unwrap_or("")
        .to_string();

    let mut req = http::Request::builder()
        .method(method)
        .uri(url)
        .header("host", &host)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body.to_vec())
        .map_err(|e| ProviderError::Other(e.to_string()))?;

    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or(req.uri().path())
        .to_string();

    let headers_iter = req
        .headers()
        .iter()
        .filter_map(|(k, v)| Some((k.as_str(), v.to_str().ok()?)));

    let signable = SignableRequest::new(
        req.method().as_str(),
        &path,
        headers_iter,
        SignableBody::Bytes(body),
    )
    .map_err(|e| ProviderError::Other(e.to_string()))?;

    let output =
        sign(signable, &signing_params).map_err(|e| ProviderError::Other(e.to_string()))?;
    let (instructions, _signature) = output.into_parts();
    instructions.apply_to_request_http1x(&mut req);
    Ok(req)
}

#[async_trait]
impl Provider for BedrockProvider {
    fn name(&self) -> &str {
        "bedrock"
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    fn pricing(&self) -> Option<Pricing> {
        None
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        let (system, messages) = messages_to_bedrock(&req);
        if messages.is_empty() {
            return Err(ProviderError::Other(
                "bedrock: no messages to send".into(),
            ));
        }

        let mut body = json!({
            "messages": messages,
            "inferenceConfig": {
                "maxTokens": req.max_tokens,
                "temperature": req.temperature,
            }
        });
        if let Some(sys) = system {
            body["system"] = json!([{ "text": sys }]);
        }
        if !req.tools.is_empty() {
            let tool_cfg: Vec<Value> = req
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "toolSpec": {
                            "name": t.function.name,
                            "description": t.function.description,
                            "inputSchema": { "json": t.function.parameters }
                        }
                    })
                })
                .collect();
            body["toolConfig"] = json!({ "tools": tool_cfg });
        }

        let body_bytes = serde_json::to_vec(&body).map_err(ProviderError::Json)?;
        // Model id may contain `:` — pass raw in path (AWS accepts unencoded colon in model id).
        let url = format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/converse",
            self.config.region, self.config.model
        );

        let signed = match signed_request(
            "POST",
            &url,
            &self.config.region,
            &self.config.access_key_id,
            &self.config.secret_access_key,
            self.config.session_token.as_deref(),
            &body_bytes,
        ) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "bedrock sign failed");
                return Err(e);
            }
        };

        let mut rb = self.client.post(url).body(body_bytes);
        for (k, v) in signed.headers().iter() {
            if let Ok(s) = v.to_str() {
                rb = rb.header(k.as_str(), s);
            }
        }

        let resp = rb
            .send()
            .await
            .map_err(|e| ProviderError::Other(format!("bedrock http: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ProviderError::Other(format!("bedrock body: {e}")))?;
        if !status.is_success() {
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: text,
            });
        }

        let v: Value = serde_json::from_str(&text).map_err(ProviderError::Json)?;
        let mut out_text = String::new();
        if let Some(arr) = v
            .pointer("/output/message/content")
            .and_then(|c| c.as_array())
        {
            for block in arr {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    out_text.push_str(t);
                }
            }
        }

        let deltas = vec![
            Ok(Delta::Text(out_text)),
            Ok(Delta::Done {
                stop_reason: StopReason::EndTurn,
            }),
        ];
        Ok(Box::pin(stream::iter(deltas)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_provider_core::Message;

    #[test]
    fn validate_rejects_empty_keys() {
        let cfg = BedrockConfig::new("m", "us-east-1", "", "secret");
        assert!(cfg.validate().is_err());
        let cfg = BedrockConfig::new("", "us-east-1", "ak", "sk");
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_ok() {
        let cfg = BedrockConfig::new(DEFAULT_MODEL, DEFAULT_REGION, "AKIAtest", "secret");
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn provider_name_model() {
        let cfg = BedrockConfig::new("my-model", "eu-west-1", "ak", "sk");
        let p = BedrockProvider::new(cfg).unwrap();
        assert_eq!(p.name(), "bedrock");
        assert_eq!(p.model(), "my-model");
    }

    #[test]
    fn messages_map_user_assistant() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![Message::user("hi"), Message::assistant("yo")],
            tools: vec![],
            max_tokens: 100,
            temperature: 0.2,
            system: Some("sys".into()),
            thinking_budget: None,
            native_web_search: false,
            native_code_execution: false,
            native_x_search: false,
            response_schema: None,
        };
        let (sys, msgs) = messages_to_bedrock(&req);
        assert_eq!(sys.as_deref(), Some("sys"));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
    }
}
