//! Voice input for harness — record audio and transcribe via Whisper.
//!
//! # Backends
//! - **OpenAI**: POST to `/v1/audio/transcriptions` with `whisper-1` model.
//! - **Local**: shell out to `whisper-cli` (whisper.cpp) if available on `$PATH`.
//!
//! # Audio capture
//! Uses system commands in order of preference:
//!   1. `sox rec` (install: `brew install sox`)
//!   2. macOS `afconvert` + `rec` via `afrecord` (built-in but limited)
//!   3. Error with install instructions if nothing is available.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

/// Transcription backend choice.
#[derive(Debug, Clone, PartialEq)]
pub enum WhisperBackend {
    /// Use OpenAI's Whisper API (requires `OPENAI_API_KEY`).
    OpenAI { api_key: String, base_url: String },
    /// Use local `whisper-cli` from whisper.cpp.
    Local { executable: String },
}

impl WhisperBackend {
    /// Auto-detect the best available backend.
    pub fn detect(openai_key: Option<&str>) -> Self {
        // Prefer local if whisper-cli is available (free, offline, fast)
        if is_available("whisper-cli") || is_available("whisper") {
            let exe = if is_available("whisper-cli") {
                "whisper-cli"
            } else {
                "whisper"
            };
            return WhisperBackend::Local {
                executable: exe.to_string(),
            };
        }
        // Fall back to OpenAI API
        if let Some(key) = openai_key {
            return WhisperBackend::OpenAI {
                api_key: key.to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
            };
        }
        // Default to OpenAI — key will be checked at transcription time
        WhisperBackend::OpenAI {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }
}

/// Record audio to a temp WAV file for `duration` seconds, then transcribe.
/// Returns the transcribed text.
pub async fn record_and_transcribe(duration: Duration, backend: &WhisperBackend) -> Result<String> {
    let wav_path = record_audio(duration).await?;
    let text = transcribe(&wav_path, backend).await?;
    // Clean up temp file
    let _ = tokio::fs::remove_file(&wav_path).await;
    Ok(text)
}

/// Record audio to a temp WAV file.
async fn record_audio(duration: Duration) -> Result<PathBuf> {
    let tmp = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile()
        .context("failed to create temp file")?;
    let path = tmp.path().to_path_buf();
    // Keep the file alive (tempfile auto-deletes on drop)
    let _ = tmp.into_temp_path(); // persist until caller deletes

    let secs = duration.as_secs().max(1).to_string();

    debug!(path = %path.display(), secs, "recording audio");

    // Try sox first (cross-platform, best quality)
    if is_available("rec") || is_available("sox") {
        let exe = if is_available("rec") { "rec" } else { "sox" };
        let mut cmd = Command::new(exe);
        if exe == "sox" {
            cmd.args(["-d", path.to_str().unwrap_or("audio.wav")]);
        } else {
            cmd.arg(path.to_str().unwrap_or("audio.wav"));
        }
        cmd.args(["rate", "16000", "channels", "1", "trim", "0", &secs]);
        let status = cmd.status().await.context("failed to run sox/rec")?;
        if status.success() {
            return Ok(path);
        }
    }

    // macOS: use afconvert pipeline via built-in CoreAudio
    if cfg!(target_os = "macos") && is_available("afrecord") {
        let status = Command::new("afrecord")
            .args([
                "-d",
                &secs,
                "-f",
                "WAVE",
                path.to_str().unwrap_or("audio.wav"),
            ])
            .status()
            .await
            .context("failed to run afrecord")?;
        if status.success() {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "No audio capture tool found. Install sox: brew install sox\n\
         Then retry: harness voice"
    )
}

/// Transcribe a WAV file using the configured backend.
pub async fn transcribe(wav_path: &std::path::Path, backend: &WhisperBackend) -> Result<String> {
    match backend {
        WhisperBackend::OpenAI { api_key, base_url } => {
            transcribe_openai(wav_path, api_key, base_url).await
        }
        WhisperBackend::Local { executable } => transcribe_local(wav_path, executable).await,
    }
}

async fn transcribe_openai(
    wav_path: &std::path::Path,
    api_key: &str,
    base_url: &str,
) -> Result<String> {
    if api_key.is_empty() {
        anyhow::bail!(
            "OPENAI_API_KEY not set. Set it or install whisper.cpp for local transcription."
        );
    }

    let url = format!("{base_url}/audio/transcriptions");
    let file_bytes = tokio::fs::read(wav_path)
        .await
        .context("reading audio file")?;
    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;
    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", "whisper-1")
        .text("response_format", "text");

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .multipart(form)
        .send()
        .await
        .context("sending to Whisper API")?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Whisper API error: {body}");
    }

    let text = resp.text().await.context("reading Whisper response")?;
    Ok(text.trim().to_string())
}

async fn transcribe_local(wav_path: &std::path::Path, executable: &str) -> Result<String> {
    let out = Command::new(executable)
        .args([
            "--model",
            "base.en",
            "--output-txt",
            "--no-timestamps",
            wav_path.to_str().unwrap_or("audio.wav"),
        ])
        .output()
        .await
        .context("running whisper.cpp")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("whisper-cli error: {stderr}");
    }

    // whisper.cpp writes a .txt file alongside the input
    let txt_path = wav_path.with_extension("txt");
    if txt_path.exists() {
        let text = tokio::fs::read_to_string(&txt_path)
            .await
            .unwrap_or_default();
        let _ = tokio::fs::remove_file(&txt_path).await;
        return Ok(text.trim().to_string());
    }

    // Fall back to stdout
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn is_available(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Returns true if any audio capture tool is available on this system.
pub fn voice_available() -> bool {
    is_available("rec")
        || is_available("sox")
        || (cfg!(target_os = "macos") && is_available("afrecord"))
}

pub mod realtime;
pub use realtime::{RealtimeEvent, RealtimeVoiceSession};
