//! Cross-machine sync for Harness state.
//!
//! Encrypts `~/.harness/{sessions.db, memory.db, trust.json, cost.db}` with
//! `age` (scrypt/passphrase) encryption and pushes/pulls to a private git repository.
//!
//! The passphrase is stored in the system keychain (macOS Keychain / libsecret on Linux)
//! via the platform `security` command-line tool, with a fallback to a local key file at
//! `~/.harness/.sync-key` (mode 0600).
//!
//! # Usage
//! ```
//! harness sync init git@github.com:user/harness-state.git
//! harness sync push
//! harness sync pull
//! harness sync status
//! harness sync auth
//! ```

use age::secrecy::SecretString;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{info, warn};

const KEYCHAIN_SERVICE: &str = "harness-sync";
const KEYCHAIN_ACCOUNT: &str = "passphrase";

/// Files to sync (relative to `~/.harness/`).
const SYNC_FILES: &[&str] = &[
    "sessions.db",
    "memory.db",
    "trust.json",
    "cost.db",
];

fn harness_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".harness")
}

fn sync_repo_dir() -> PathBuf {
    harness_dir().join("sync-repo")
}

fn sync_config_path() -> PathBuf {
    harness_dir().join("sync.json")
}

fn key_file_path() -> PathBuf {
    harness_dir().join(".sync-key")
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SyncConfig {
    git_url: String,
}

fn load_sync_config() -> Result<SyncConfig> {
    let path = sync_config_path();
    if !path.exists() {
        anyhow::bail!("Sync not initialised. Run: harness sync init <git-url>");
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

fn save_sync_config(cfg: &SyncConfig) -> Result<()> {
    let _ = std::fs::create_dir_all(harness_dir());
    std::fs::write(sync_config_path(), serde_json::to_string_pretty(cfg)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Passphrase management (macOS Keychain → local key file fallback)
// ---------------------------------------------------------------------------

/// Store `passphrase` in macOS Keychain using the `security` CLI.
async fn keychain_set(passphrase: &str) -> bool {
    Command::new("security")
        .args([
            "add-generic-password",
            "-s", KEYCHAIN_SERVICE,
            "-a", KEYCHAIN_ACCOUNT,
            "-w", passphrase,
            "-U",
        ])
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Retrieve passphrase from macOS Keychain.
async fn keychain_get() -> Option<String> {
    let out = Command::new("security")
        .args([
            "find-generic-password",
            "-s", KEYCHAIN_SERVICE,
            "-a", KEYCHAIN_ACCOUNT,
            "-w",
        ])
        .output()
        .await
        .ok()?;
    if out.status.success() {
        let pw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !pw.is_empty() { Some(pw) } else { None }
    } else {
        None
    }
}

/// Get or create the sync passphrase. Tries keychain first, then the key file.
pub async fn get_or_create_passphrase() -> Result<String> {
    // Try keychain first
    if let Some(pw) = keychain_get().await {
        return Ok(pw);
    }

    // Try local key file
    let kf = key_file_path();
    if kf.exists() {
        let pw = std::fs::read_to_string(&kf)?.trim().to_string();
        if !pw.is_empty() {
            return Ok(pw);
        }
    }

    // Generate a new passphrase
    let passphrase = generate_passphrase();

    // Try to store in keychain
    if keychain_get().await.is_none() {
        if keychain_set(&passphrase).await {
            info!("Stored sync passphrase in macOS Keychain");
        } else {
            // Fallback: write to local file
            warn!("Keychain unavailable — storing passphrase in ~/.harness/.sync-key");
            let _ = std::fs::create_dir_all(harness_dir());
            std::fs::write(&kf, &passphrase)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&kf, std::fs::Permissions::from_mode(0o600))?;
            }
        }
    }

    Ok(passphrase)
}

fn generate_passphrase() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "hrns-{:x}-{:x}",
        t.as_secs(),
        t.subsec_nanos() ^ 0xdeadbeef
    )
}

// ---------------------------------------------------------------------------
// Encryption / decryption (age scrypt)
// ---------------------------------------------------------------------------

fn encrypt_bytes(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let recipient = age::scrypt::Recipient::new(SecretString::new(passphrase.to_owned().into()));
    age::encrypt(&recipient, data).context("age encrypt")
}

fn decrypt_bytes(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let identity = age::scrypt::Identity::new(SecretString::new(passphrase.to_owned().into()));
    age::decrypt(&identity, data).context("age decrypt")
}

// ---------------------------------------------------------------------------
// Tar helpers (memory/ directory)
// ---------------------------------------------------------------------------

fn tar_dir(dir: &Path) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut buf);
        builder.append_dir_all("memory", dir)?;
        builder.finish()?;
    }
    Ok(buf)
}

fn untar_dir(data: &[u8], parent: &Path) -> Result<()> {
    std::fs::create_dir_all(parent)?;
    let mut archive = tar::Archive::new(data);
    archive.unpack(parent)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public commands
// ---------------------------------------------------------------------------

/// Initialise sync: save config and clone/init the remote git repo.
pub async fn init(git_url: &str) -> Result<()> {
    let repo_dir = sync_repo_dir();

    if repo_dir.exists() {
        let out = Command::new("git")
            .args(["-C", &repo_dir.to_string_lossy(), "remote", "get-url", "origin"])
            .output()
            .await?;
        let existing = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if existing == git_url {
            println!("Sync repo already configured: {git_url}");
            return Ok(());
        }
        anyhow::bail!(
            "Sync repo exists with different remote: {existing}\n\
             Remove {} to reinitialise.",
            repo_dir.display()
        );
    }

    // Try to clone; if it fails (empty remote), init + set-remote
    let status = Command::new("git")
        .args(["clone", git_url, &repo_dir.to_string_lossy()])
        .status()
        .await?;

    if !status.success() {
        std::fs::create_dir_all(&repo_dir)?;
        Command::new("git").args(["-C", &repo_dir.to_string_lossy(), "init"]).status().await?;
        Command::new("git")
            .args(["-C", &repo_dir.to_string_lossy(), "remote", "add", "origin", git_url])
            .status()
            .await?;
    }

    save_sync_config(&SyncConfig { git_url: git_url.to_string() })?;
    let _ = get_or_create_passphrase().await?;

    println!("✓ Sync initialised → {git_url}");
    println!("  Passphrase stored in system keychain (or ~/.harness/.sync-key).");
    println!("  Run `harness sync push` to upload your state.");
    Ok(())
}

/// Encrypt and push harness state to the remote git repo.
pub async fn push() -> Result<()> {
    let cfg = load_sync_config()?;
    let passphrase = get_or_create_passphrase().await?;
    let repo_dir = sync_repo_dir();
    let src_dir = harness_dir();

    // Pull first to avoid trivial conflicts
    let _ = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "pull", "--rebase", "origin", "main"])
        .status()
        .await;

    let mut count = 0usize;
    for name in SYNC_FILES {
        let src = src_dir.join(name);
        if !src.exists() {
            continue;
        }
        let data = std::fs::read(&src).with_context(|| format!("reading {name}"))?;
        let enc = encrypt_bytes(&data, &passphrase)?;
        std::fs::write(repo_dir.join(format!("{name}.age")), enc)?;
        count += 1;
    }

    // memory/ directory as tarball
    let mem = src_dir.join("memory");
    if mem.exists() {
        if let Ok(tar) = tar_dir(&mem) {
            std::fs::write(repo_dir.join("memory.tar.age"), encrypt_bytes(&tar, &passphrase)?)?;
            count += 1;
        }
    }

    if count == 0 {
        println!("Nothing to push (no state files found in ~/.harness/).");
        return Ok(());
    }

    Command::new("git").args(["-C", &repo_dir.to_string_lossy(), "add", "."]).status().await?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let msg = format!("harness sync push {now}");
    let committed = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "commit", "-m", &msg])
        .status()
        .await?;

    if committed.success() {
        Command::new("git")
            .args(["-C", &repo_dir.to_string_lossy(), "push", "-u", "origin", "main"])
            .status()
            .await?;
        println!("✓ Pushed {count} file(s) to {}", cfg.git_url);
    } else {
        println!("Already up to date — nothing to commit.");
    }

    Ok(())
}

/// Pull and decrypt harness state from the remote git repo.
pub async fn pull() -> Result<()> {
    let _cfg = load_sync_config()?;
    let passphrase = get_or_create_passphrase().await?;
    let repo_dir = sync_repo_dir();
    let dst_dir = harness_dir();

    let status = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "pull", "origin", "main"])
        .status()
        .await?;
    if !status.success() {
        warn!("git pull returned non-zero — using cached copy");
    }

    let mut count = 0usize;
    for name in SYNC_FILES {
        let src = repo_dir.join(format!("{name}.age"));
        if !src.exists() {
            continue;
        }
        let enc = std::fs::read(&src)?;
        let data = decrypt_bytes(&enc, &passphrase).with_context(|| format!("decrypting {name}"))?;
        std::fs::write(dst_dir.join(name), data)?;
        count += 1;
    }

    // Restore memory/
    let mem_age = repo_dir.join("memory.tar.age");
    if mem_age.exists() {
        let enc = std::fs::read(&mem_age)?;
        let tar = decrypt_bytes(&enc, &passphrase)?;
        untar_dir(&tar, &dst_dir)?;
        count += 1;
    }

    if count == 0 {
        println!("Nothing to pull (remote is empty).");
    } else {
        println!("✓ Pulled {count} file(s) to ~/.harness/");
    }

    Ok(())
}

/// Show sync status.
pub async fn status() -> Result<()> {
    let cfg = load_sync_config()?;
    println!("Sync remote : {}", cfg.git_url);
    let repo_dir = sync_repo_dir();
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "log", "--oneline", "-5"])
        .output()
        .await?;
    let log = String::from_utf8_lossy(&out.stdout);
    if log.trim().is_empty() {
        println!("No commits yet — run `harness sync push` first.");
    } else {
        println!("Recent syncs:\n{log}");
    }
    Ok(())
}
