//! OpenAI Realtime API WebSocket client for duplex voice conversations.
//!
//! Connects to `wss://api.openai.com/v1/realtime?model=gpt-4o-realtime-preview`
//! and streams audio in both directions — mic → API → speakers.
//!
//! Audio capture uses sox/rec (same as the batch voice module). TTS playback
//! uses `aplay` (Linux) or `afplay` (macOS) on a pipe.
//!
//! See: <https://platform.openai.com/docs/guides/realtime>

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message as WsMsg};
use tracing::{debug, warn};

// ── Public API ────────────────────────────────────────────────────────────────

/// Events emitted by the realtime session to the caller (TUI, etc.).
#[derive(Debug, Clone)]
pub enum RealtimeEvent {
    /// Assistant is speaking — partial text transcript.
    AssistantTranscript(String),
    /// Assistant speech audio chunk (PCM16 bytes).
    AudioChunk(Vec<u8>),
    /// Turn complete: full assistant transcript.
    TurnComplete(String),
    /// User speech detected (VAD barge-in: agent stopped speaking).
    UserSpeechStart,
    /// Error from API.
    Error(String),
}

/// Handle to an active realtime voice session.
pub struct RealtimeVoiceSession {
    /// Send audio bytes captured from the microphone.
    pub audio_tx: mpsc::Sender<Vec<u8>>,
    /// Receive events from the session.
    pub event_rx: mpsc::Receiver<RealtimeEvent>,
    /// Close the session.
    _shutdown: tokio::task::JoinHandle<()>,
}

impl RealtimeVoiceSession {
    /// Start a realtime session against the OpenAI Realtime API.
    /// `api_key` must be a valid OPENAI_API_KEY.
    pub async fn connect(api_key: &str, system_prompt: &str) -> Result<Self> {
        let url = "wss://api.openai.com/v1/realtime?model=gpt-4o-realtime-preview-2025-06-03";
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<u8>>(64);
        let (event_tx, event_rx) = mpsc::channel::<RealtimeEvent>(256);

        let key = api_key.to_string();
        let sysprompt = system_prompt.to_string();

        let handle = tokio::spawn(async move {
            if let Err(e) = run_session(url, &key, &sysprompt, audio_rx, event_tx.clone()).await {
                let _ = event_tx.send(RealtimeEvent::Error(e.to_string())).await;
            }
        });

        Ok(Self {
            audio_tx,
            event_rx,
            _shutdown: handle,
        })
    }

    /// Send raw PCM16LE audio bytes captured from the microphone.
    pub async fn send_audio(&mut self, pcm: Vec<u8>) -> Result<()> {
        self.audio_tx.send(pcm).await.context("session closed")
    }

    /// Commit the current audio buffer (tell API the utterance is complete).
    pub async fn commit(&mut self) -> Result<()> {
        // Sentinel: empty vec signals commit
        self.audio_tx.send(vec![]).await.context("session closed")
    }
}

// ── Internal session driver ───────────────────────────────────────────────────

async fn run_session(
    url: &str,
    api_key: &str,
    system_prompt: &str,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
    event_tx: mpsc::Sender<RealtimeEvent>,
) -> Result<()> {
    let request = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
        .uri(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("OpenAI-Beta", "realtime=v1")
        .body(())?;

    let (ws, _) = connect_async_tls_with_config(request, None, true, None)
        .await
        .context("WebSocket connect")?;

    let (mut ws_tx, mut ws_rx) = ws.split();

    // Send session.update to configure the session
    let session_cfg = json!({
        "type": "session.update",
        "session": {
            "modalities": ["text", "audio"],
            "instructions": system_prompt,
            "voice": "alloy",
            "input_audio_format": "pcm16",
            "output_audio_format": "pcm16",
            "input_audio_transcription": { "model": "whisper-1" },
            "turn_detection": {
                "type": "server_vad",
                "threshold": 0.5,
                "prefix_padding_ms": 300,
                "silence_duration_ms": 500
            }
        }
    });
    ws_tx.send(WsMsg::Text(session_cfg.to_string())).await?;
    debug!("realtime: session configured");

    // Run send and receive concurrently
    let ev_tx2 = event_tx.clone();
    tokio::select! {
        _ = async {
            // Send audio from mic to API
            while let Some(audio) = audio_rx.recv().await {
                if audio.is_empty() {
                    // Commit signal
                    let commit = json!({ "type": "input_audio_buffer.commit" });
                    if ws_tx.send(WsMsg::Text(commit.to_string())).await.is_err() { break; }
                } else {
                    let b64 = B64.encode(&audio);
                    let append = json!({
                        "type": "input_audio_buffer.append",
                        "audio": b64
                    });
                    if ws_tx.send(WsMsg::Text(append.to_string())).await.is_err() { break; }
                }
            }
        } => {}

        _ = async {
            // Receive events from API
            let mut transcript_buf = String::new();
            while let Some(msg) = ws_rx.next().await {
                let text = match msg {
                    Ok(WsMsg::Text(t)) => t,
                    Ok(WsMsg::Close(_)) => break,
                    _ => continue,
                };
                if let Ok(ev) = serde_json::from_str::<serde_json::Value>(&text) {
                    match ev["type"].as_str() {
                        Some("response.audio.delta") => {
                            if let Some(b64) = ev["delta"].as_str() {
                                if let Ok(bytes) = B64.decode(b64) {
                                    let _ = ev_tx2.send(RealtimeEvent::AudioChunk(bytes)).await;
                                }
                            }
                        }
                        Some("response.audio_transcript.delta") => {
                            if let Some(delta) = ev["delta"].as_str() {
                                transcript_buf.push_str(delta);
                                let _ = ev_tx2.send(RealtimeEvent::AssistantTranscript(transcript_buf.clone())).await;
                            }
                        }
                        Some("response.done") => {
                            let full = std::mem::take(&mut transcript_buf);
                            let _ = ev_tx2.send(RealtimeEvent::TurnComplete(full)).await;
                        }
                        Some("input_audio_buffer.speech_started") => {
                            let _ = ev_tx2.send(RealtimeEvent::UserSpeechStart).await;
                        }
                        Some("error") => {
                            let msg = ev["error"]["message"].as_str().unwrap_or("unknown error").to_string();
                            warn!("realtime API error: {msg}");
                            let _ = ev_tx2.send(RealtimeEvent::Error(msg)).await;
                        }
                        _ => {}
                    }
                }
            }
        } => {}
    }

    Ok(())
}

// ── Audio playback helper ─────────────────────────────────────────────────────

/// Play raw PCM16LE audio bytes through the system speaker.
/// Sample rate: 24000 Hz, mono.
pub async fn play_pcm16(pcm: &[u8]) -> Result<()> {
    let tmp = tempfile::Builder::new().suffix(".raw").tempfile()?;
    let path = tmp.path().to_path_buf();
    tokio::fs::write(&path, pcm).await?;
    let _ = tmp.into_temp_path(); // keep alive

    if cfg!(target_os = "macos") {
        // macOS: convert raw PCM to AIFF and play with afplay
        let aiff_path = path.with_extension("aiff");
        tokio::process::Command::new("sox")
            .args([
                "-r",
                "24000",
                "-e",
                "signed-integer",
                "-b",
                "16",
                "-c",
                "1",
                path.to_str().unwrap_or(""),
                aiff_path.to_str().unwrap_or(""),
            ])
            .status()
            .await
            .ok();
        tokio::process::Command::new("afplay")
            .arg(aiff_path.to_str().unwrap_or(""))
            .status()
            .await
            .ok();
    } else {
        // Linux: aplay
        tokio::process::Command::new("aplay")
            .args([
                "-r",
                "24000",
                "-f",
                "S16_LE",
                "-c",
                "1",
                path.to_str().unwrap_or(""),
            ])
            .status()
            .await
            .ok();
    }

    Ok(())
}
