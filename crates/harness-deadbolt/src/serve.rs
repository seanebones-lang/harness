//! Local Unix sidecar. Not a model tool. Does not change admit semantics.
//!
//! `kill`, `pause`, `clip`, and `resume` stay on the CLI. This process only
//! forwards admit, ensure, register_child, and status to the same store.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::{AdmitDecision, Deadbolt, DeadboltConfig, DeadboltError};

/// Default bind. Local socket, not a TCP address.
pub fn default_bind_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".harness")
        .join("deadbolt.sock")
}

/// True when the bind must be refused.
///
/// Unix paths are allowed. `0.0.0.0`, `[::]`, and any TCP `host:port`
/// (including non-loopback) are not.
pub fn bind_refused(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    if raw.contains("0.0.0.0") || raw.contains("[::]") {
        return true;
    }
    tcp_bind(&raw)
}

fn tcp_bind(raw: &str) -> bool {
    if raw.contains('/') || raw.contains('\\') {
        return false;
    }
    if let Some(rest) = raw.strip_prefix('[') {
        if let Some((_, port)) = rest.split_once("]:") {
            return !port.is_empty() && port.chars().all(|c| c.is_ascii_digit());
        }
        return false;
    }
    if let Some((_, port)) = raw.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Block on `bind`, serving the store from `cfg`. Same db and JSONL as the CLI.
pub fn serve(cfg: &DeadboltConfig, bind: &Path) -> Result<(), DeadboltError> {
    if bind_refused(bind) {
        return Err(DeadboltError::BindRefused);
    }
    listen(Deadbolt::open(cfg), bind, resolve_token(cfg)?)
}

fn resolve_token(cfg: &DeadboltConfig) -> Result<Option<String>, DeadboltError> {
    if let Some(name) = cfg.token_env.as_deref() {
        if let Ok(raw) = std::env::var(name) {
            let value = raw.trim();
            if !value.is_empty() {
                return Ok(Some(value.to_string()));
            }
        }
    }
    let Some(path) = cfg.token_file.as_deref() else {
        return Ok(None);
    };
    let path = Path::new(path);
    if !path.exists() {
        return Ok(None);
    }
    let mode = fs::metadata(path)
        .map_err(|_| DeadboltError::BindRefused)?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o600 {
        return Err(DeadboltError::BindRefused);
    }
    let raw = fs::read_to_string(path).map_err(|_| DeadboltError::BindRefused)?;
    let value = raw.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

fn listen(gate: Deadbolt, bind: &Path, token: Option<String>) -> Result<(), DeadboltError> {
    if let Some(parent) = bind.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|_| DeadboltError::BindRefused)?;
        }
    }
    let _ = fs::remove_file(bind);
    let listener = UnixListener::bind(bind).map_err(|_| DeadboltError::BindRefused)?;
    let _ = fs::set_permissions(bind, fs::Permissions::from_mode(0o600));
    for conn in listener.incoming() {
        let Ok(stream) = conn else {
            continue;
        };
        let gate = gate.clone();
        let token = token.clone();
        thread::spawn(move || {
            let _ = handle_stream(&gate, stream, token.as_deref());
        });
    }
    Ok(())
}

fn handle_stream(
    gate: &Deadbolt,
    mut stream: UnixStream,
    token: Option<&str>,
) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(header_end) = find_header_end(&buf) {
            let header = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let need = content_length(&header);
            let have = buf.len().saturating_sub(header_end + 4);
            if have >= need {
                break;
            }
        }
        if buf.len() > 65_536 {
            break;
        }
    }
    let raw = String::from_utf8_lossy(&buf).to_string();
    let (status, body) = dispatch_http(gate, &raw, token);
    let resp = format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes())?;
    Ok(())
}

fn header_value(raw: &str, name: &str) -> Option<String> {
    let headers = raw.split_once("\r\n\r\n").map(|(h, _)| h).unwrap_or(raw);
    for line in headers.lines().skip(1) {
        let (key, value) = line.split_once(':')?;
        if key.eq_ignore_ascii_case(name) {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn token_eq(got: &str, expected: &str) -> bool {
    let got = got.as_bytes();
    let expected = expected.as_bytes();
    if got.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in got.iter().zip(expected) {
        diff |= a ^ b;
    }
    diff == 0
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn content_length(header: &str) -> usize {
    header
        .lines()
        .find_map(|line| {
            let (k, v) = line.split_once(':')?;
            if k.eq_ignore_ascii_case("content-length") {
                v.trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn dispatch_http(gate: &Deadbolt, raw: &str, token: Option<&str>) -> (u16, String) {
    if let Some(expected) = token {
        match header_value(raw, "x-deadbolt-token") {
            Some(got) if token_eq(&got, expected) => {}
            _ => return (401, json!({"code":"unauthorized"}).to_string()),
        }
    }
    let mut lines = raw.split("\r\n");
    let Some(req) = lines.next() else {
        return (400, json!({"code":"bad_request"}).to_string());
    };
    let mut parts = req.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let body = raw.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    match (method, path) {
        ("POST", "/admit") => admit(gate, body),
        ("POST", "/ensure") => ensure(gate, body),
        ("POST", "/register_child") => register_child(gate, body),
        ("GET", "/status") => status(gate, query),
        _ => (404, json!({"code":"not_found"}).to_string()),
    }
}

fn admit(gate: &Deadbolt, body: &str) -> (u16, String) {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return (400, json!({"code":"bad_request"}).to_string());
    };
    let Some(agent_id) = v.get("agent_id").and_then(|x| x.as_str()) else {
        return (400, json!({"code":"bad_request"}).to_string());
    };
    let Some(tool) = v.get("tool").and_then(|x| x.as_str()) else {
        return (400, json!({"code":"bad_request"}).to_string());
    };
    match gate.admit(agent_id, tool) {
        AdmitDecision::Allow => (200, json!({"decision":"allow"}).to_string()),
        AdmitDecision::Deny { code } => (
            200,
            json!({"decision":"deny","code": code.as_str()}).to_string(),
        ),
    }
}

fn ensure(gate: &Deadbolt, body: &str) -> (u16, String) {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return (400, json!({"code":"bad_request"}).to_string());
    };
    let Some(agent_id) = v.get("agent_id").and_then(|x| x.as_str()) else {
        return (400, json!({"code":"bad_request"}).to_string());
    };
    match gate.ensure_agent(agent_id) {
        Ok(()) => (200, json!({"ok":true}).to_string()),
        Err(e) => (200, json!({"ok":false,"code": err_token(&e)}).to_string()),
    }
}

fn register_child(gate: &Deadbolt, body: &str) -> (u16, String) {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return (400, json!({"code":"bad_request"}).to_string());
    };
    let Some(parent) = v.get("parent").and_then(|x| x.as_str()) else {
        return (400, json!({"code":"bad_request"}).to_string());
    };
    let Some(child) = v.get("child").and_then(|x| x.as_str()) else {
        return (400, json!({"code":"bad_request"}).to_string());
    };
    let swarm = v.get("swarm_task_id").and_then(|x| x.as_str());
    match gate.register_child(parent, child, swarm) {
        Ok(live) => (200, json!({"live": live}).to_string()),
        Err(e) => (200, json!({"ok":false,"code": err_token(&e)}).to_string()),
    }
}

fn status(gate: &Deadbolt, query: &str) -> (u16, String) {
    let agent = query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k == "agent" && !v.is_empty() {
            Some(v)
        } else {
            None
        }
    });
    match gate.status(agent) {
        Ok(rows) => {
            let agents: Vec<Value> = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "agent_id": row.agent_id,
                        "state": row.state,
                        "parent_id": row.parent_id,
                        "expires_at": row.expires_at,
                        "clips": row.clips,
                        "swarm_task_id": row.swarm_task_id,
                    })
                })
                .collect();
            (200, json!({"agents": agents}).to_string())
        }
        Err(e) => (200, json!({"ok":false,"code": err_token(&e)}).to_string()),
    }
}

fn err_token(err: &DeadboltError) -> &'static str {
    match err {
        DeadboltError::StoreUnavailable => "store_unavailable",
        DeadboltError::Disabled => "disabled",
        DeadboltError::NotFound => "not_found",
        DeadboltError::Killed => "killed",
        DeadboltError::DrillFailed(_) => "drill_failed",
        DeadboltError::BindRefused => "bind_refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeadboltConfig;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn sock_dir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(1);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("dbs{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cfg_at(dir: &Path) -> DeadboltConfig {
        DeadboltConfig {
            db_path: Some(dir.join("deadbolt.db").display().to_string()),
            events_path: Some(dir.join("events.jsonl").display().to_string()),
            ..DeadboltConfig::default()
        }
    }

    fn start_with(gate: Deadbolt, sock: PathBuf, token: Option<String>) {
        let listen_at = sock.clone();
        thread::spawn(move || {
            let _ = listen(gate, &listen_at, token);
        });
        for _ in 0..50 {
            if sock.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("socket did not appear");
    }

    fn start(gate: Deadbolt, sock: PathBuf) {
        start_with(gate, sock, None);
    }

    fn http(sock: &Path, method: &str, path: &str, body: Option<&str>) -> Value {
        let body = body.unwrap_or("");
        let req = format!(
            "{method} {path} HTTP/1.1\r\nhost: local\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut last = String::new();
        for _ in 0..20 {
            if let Ok(mut stream) = UnixStream::connect(sock) {
                stream.write_all(req.as_bytes()).unwrap();
                let _ = stream.shutdown(std::net::Shutdown::Write);
                last.clear();
                stream.read_to_string(&mut last).unwrap();
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let json = last.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        serde_json::from_str(json).unwrap_or_else(|_| json!({"raw": last}))
    }

    #[test]
    fn bind_refuses_wildcard() {
        assert!(bind_refused(Path::new("0.0.0.0")));
        assert!(bind_refused(Path::new("0.0.0.0:9")));
        assert!(bind_refused(Path::new("192.168.1.10:9")));
        assert!(bind_refused(Path::new("10.0.0.1:8080")));
        assert!(bind_refused(Path::new("[::]:9")));
        assert!(bind_refused(Path::new("127.0.0.1:9")));
        assert!(!bind_refused(Path::new("/tmp/deadbolt.sock")));
        assert!(serve(&DeadboltConfig::default(), Path::new("192.168.1.10:9")).is_err());
    }

    fn http_status(
        sock: &Path,
        method: &str,
        path: &str,
        body: Option<&str>,
        token: Option<&str>,
    ) -> (u16, Value) {
        let body = body.unwrap_or("");
        let token_line = token
            .map(|t| format!("x-deadbolt-token: {t}\r\n"))
            .unwrap_or_default();
        let req = format!(
            "{method} {path} HTTP/1.1\r\nhost: local\r\n{token_line}content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut last = String::new();
        for _ in 0..20 {
            if let Ok(mut stream) = UnixStream::connect(sock) {
                stream.write_all(req.as_bytes()).unwrap();
                let _ = stream.shutdown(std::net::Shutdown::Write);
                last.clear();
                stream.read_to_string(&mut last).unwrap();
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let status = last
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let json = last.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        let value = serde_json::from_str(json).unwrap_or_else(|_| json!({"raw": last}));
        (status, value)
    }

    #[test]
    fn socket_mode_is_0600() {
        let dir = sock_dir();
        let sock = dir.join("s");
        let gate = Deadbolt::open(&cfg_at(&dir));
        start(gate, sock.clone());
        let mode = fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn serve_with_token_rejects_missing_header() {
        let dir = sock_dir();
        let sock = dir.join("s");
        let gate = Deadbolt::open(&cfg_at(&dir));
        start_with(gate.clone(), sock.clone(), Some("s3cret".into()));
        let (status, body) =
            http_status(&sock, "POST", "/ensure", Some(r#"{"agent_id":"A"}"#), None);
        assert_eq!(status, 401);
        assert_eq!(body["code"], "unauthorized");
        let (wrong, _) = http_status(
            &sock,
            "POST",
            "/admit",
            Some(r#"{"agent_id":"A","tool":"shell"}"#),
            Some("nope"),
        );
        assert_eq!(wrong, 401);
        let (status_get, _) = http_status(&sock, "GET", "/status", None, None);
        assert_eq!(status_get, 401);
        let (child, _) = http_status(
            &sock,
            "POST",
            "/register_child",
            Some(r#"{"parent":"A","child":"C"}"#),
            None,
        );
        assert_eq!(child, 401);
        assert!(gate.status(Some("A")).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn serve_with_token_accepts_matching_header_and_admit_still_killed_after_kill() {
        let dir = sock_dir();
        let sock = dir.join("s");
        let gate = Deadbolt::open(&cfg_at(&dir));
        start_with(gate.clone(), sock.clone(), Some("s3cret".into()));
        let (status, _) = http_status(
            &sock,
            "POST",
            "/ensure",
            Some(r#"{"agent_id":"A"}"#),
            Some("s3cret"),
        );
        assert_eq!(status, 200);
        let (status, allowed) = http_status(
            &sock,
            "POST",
            "/admit",
            Some(r#"{"agent_id":"A","tool":"shell"}"#),
            Some("s3cret"),
        );
        assert_eq!(status, 200);
        assert_eq!(allowed["decision"], "allow");
        gate.kill("A").unwrap();
        let (status, denied) = http_status(
            &sock,
            "POST",
            "/admit",
            Some(r#"{"agent_id":"A","tool":"shell"}"#),
            Some("s3cret"),
        );
        assert_eq!(status, 200);
        assert_eq!(denied["decision"], "deny");
        assert_eq!(denied["code"], "killed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn token_file_not_0600_is_refused() {
        let dir = sock_dir();
        let file = dir.join("token");
        fs::write(&file, "abc\n").unwrap();
        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&file, perms.clone()).unwrap();
        let mut cfg = cfg_at(&dir);
        cfg.token_file = Some(file.display().to_string());
        assert!(resolve_token(&cfg).is_err());
        perms.set_mode(0o600);
        fs::set_permissions(&file, perms).unwrap();
        assert_eq!(resolve_token(&cfg).unwrap().as_deref(), Some("abc"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sidecar_post_kill_admit_is_killed() {
        let dir = sock_dir();
        let sock = dir.join("s");
        let cfg = cfg_at(&dir);
        let gate = Deadbolt::open(&cfg);
        start(gate.clone(), sock.clone());
        assert_eq!(
            http(&sock, "POST", "/ensure", Some(r#"{"agent_id":"A"}"#))["ok"],
            true
        );
        assert_eq!(
            http(&sock, "POST", "/ensure", Some(r#"{"agent_id":"B"}"#))["ok"],
            true
        );
        assert_eq!(
            http(
                &sock,
                "POST",
                "/admit",
                Some(r#"{"agent_id":"A","tool":"shell"}"#)
            )["decision"],
            "allow"
        );
        gate.kill("A").unwrap();
        let denied = http(
            &sock,
            "POST",
            "/admit",
            Some(r#"{"agent_id":"A","tool":"shell"}"#),
        );
        assert_eq!(denied["decision"], "deny");
        assert_eq!(denied["code"], "killed");
        assert_eq!(
            http(
                &sock,
                "POST",
                "/admit",
                Some(r#"{"agent_id":"B","tool":"shell"}"#)
            )["decision"],
            "allow"
        );
        let kill = http(&sock, "POST", "/kill", Some(r#"{"agent_id":"B"}"#));
        assert_eq!(kill["code"], "not_found");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sidecar_parent_kill_denies_child_admit() {
        let dir = sock_dir();
        let sock = dir.join("s");
        let cfg = cfg_at(&dir);
        let gate = Deadbolt::open(&cfg);
        start(gate.clone(), sock.clone());
        http(&sock, "POST", "/ensure", Some(r#"{"agent_id":"P"}"#));
        http(&sock, "POST", "/ensure", Some(r#"{"agent_id":"B"}"#));
        let reg = http(
            &sock,
            "POST",
            "/register_child",
            Some(r#"{"parent":"P","child":"C","swarm_task_id":"swarm-side"}"#),
        );
        assert_eq!(reg["live"], true);
        assert_eq!(
            http(
                &sock,
                "POST",
                "/admit",
                Some(r#"{"agent_id":"C","tool":"read_file"}"#)
            )["decision"],
            "allow"
        );
        gate.kill("P").unwrap();
        let denied = http(
            &sock,
            "POST",
            "/admit",
            Some(r#"{"agent_id":"C","tool":"shell"}"#),
        );
        assert_eq!(denied["decision"], "deny");
        assert_eq!(denied["code"], "killed");
        assert_eq!(
            http(
                &sock,
                "POST",
                "/admit",
                Some(r#"{"agent_id":"B","tool":"shell"}"#)
            )["decision"],
            "allow"
        );
        let listed = http(&sock, "GET", "/status", None);
        let agents = listed["agents"].as_array().unwrap();
        assert!(agents.iter().any(|row| {
            row["agent_id"] == "C" && row["state"] == "killed" && row["parent_id"] == "P"
        }));
        let _ = fs::remove_dir_all(&dir);
    }

    fn python_json(sock: &Path, args: &[&str]) -> Value {
        let client =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/deadbolt_client.py");
        let out = std::process::Command::new("python3")
            .arg(client)
            .args(args)
            .env("DEADBOLT_SOCK", sock)
            .output()
            .expect("python3");
        assert!(
            out.status.success(),
            "python failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("client json")
    }

    #[test]
    fn sidecar_python_client() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            eprintln!("sidecar_python_client: python3 missing");
            return;
        }
        let dir = sock_dir();
        let sock = dir.join("s");
        let cfg = cfg_at(&dir);
        let gate = Deadbolt::open(&cfg);
        start(gate.clone(), sock.clone());
        assert_eq!(python_json(&sock, &["ensure", "--agent", "A"])["ok"], true);
        assert_eq!(python_json(&sock, &["ensure", "--agent", "B"])["ok"], true);
        assert_eq!(
            python_json(&sock, &["admit", "--agent", "A", "--tool", "shell"])["decision"],
            "allow"
        );
        gate.kill("A").unwrap();
        let denied = python_json(&sock, &["admit", "--agent", "A", "--tool", "shell"]);
        assert_eq!(denied["decision"], "deny");
        assert_eq!(denied["code"], "killed");
        assert_eq!(
            python_json(&sock, &["admit", "--agent", "B", "--tool", "shell"])["decision"],
            "allow"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn python_missing() -> bool {
        std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
    }

    #[test]
    fn sample_agent_killed() {
        if python_missing() {
            eprintln!("sample_agent_killed: python3 missing");
            return;
        }
        let dir = sock_dir();
        let sock = dir.join("s");
        let cfg = cfg_at(&dir);
        let gate = Deadbolt::open(&cfg);
        start(gate.clone(), sock.clone());
        let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/deadbolt_sample_agent.py");
        let mut child = std::process::Command::new("python3")
            .arg(&agent)
            .args(["--agent", "samp-a", "--tool", "shell", "--interval", "0.05"])
            .env("DEADBOLT_SOCK", &sock)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("sample agent");
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        std::io::BufRead::read_line(&mut reader, &mut line).expect("first line");
        let first: Value = serde_json::from_str(line.trim()).expect("allow json");
        assert_eq!(first["decision"], "allow");
        gate.kill("samp-a").unwrap();
        line.clear();
        std::io::BufRead::read_line(&mut reader, &mut line).expect("deny line");
        let denied: Value = serde_json::from_str(line.trim()).expect("deny json");
        assert_eq!(denied["decision"], "deny");
        assert_eq!(denied["code"], "killed");
        let status = child.wait().expect("wait a");
        assert_eq!(status.code(), Some(2));

        let mut other = std::process::Command::new("python3")
            .arg(&agent)
            .args(["--agent", "samp-b", "--tool", "shell", "--interval", "0.05"])
            .env("DEADBOLT_SOCK", &sock)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("sample b");
        let bout = other.stdout.take().expect("stdout b");
        let mut breader = std::io::BufReader::new(bout);
        let mut bline = String::new();
        std::io::BufRead::read_line(&mut breader, &mut bline).expect("b line");
        let live: Value = serde_json::from_str(bline.trim()).expect("b json");
        assert_eq!(live["decision"], "allow");
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &other.id().to_string()])
            .status();
        let bstatus = other.wait().expect("wait b");
        assert!(bstatus.success());
        let _ = fs::remove_dir_all(&dir);
    }
}
