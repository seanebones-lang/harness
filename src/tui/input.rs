//! TUI keyboard, mouse, voice, search, help, and slash-command handling.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers, MouseEvent, MouseEventKind};
use harness_memory::{Session, SessionStore};
use harness_provider_core::ArcProvider;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::events::{try_emit, AgentEvent};
use crate::{agent, background, checkpoint, cost, memory_project, notifications};

use super::slash::detect_test_command;
use super::AppState;
use super::PendingConfirm;
use harness_tools::ConfirmResult;

pub(crate) fn handle_char(state: &Arc<Mutex<AppState>>, c: char) {
    state.lock().insert_char(c);
}

pub(crate) fn handle_mouse(state: &Arc<Mutex<AppState>>, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            state.lock().scroll_chat_up(3);
        }
        MouseEventKind::ScrollDown => {
            state.lock().scroll_chat_down(3);
        }
        _ => {}
    }
}

pub(crate) fn handle_voice(state: &Arc<Mutex<AppState>>) {
    let busy = state.lock().busy;
    let recording = state.lock().recording_voice;
    if busy || recording {
        state.lock().push_event("[voice] busy, please wait.");
        return;
    }
    {
        let mut st = state.lock();
        st.recording_voice = true;
        st.status = "Recording… (5s) Ctrl+S to cancel".to_string();
        st.push_event("[voice] recording 5s…");
    }
    let state2 = state.clone();
    let openai_key = std::env::var("OPENAI_API_KEY").ok();
    tokio::spawn(async move {
        use harness_voice::{record_and_transcribe, WhisperBackend};
        let backend = WhisperBackend::detect(openai_key.as_deref());
        let result = record_and_transcribe(Duration::from_secs(5), &backend).await;
        let mut st = state2.lock();
        st.recording_voice = false;
        match result {
            Ok(t) if !t.is_empty() => {
                st.input.push_str(&t);
                st.cursor_pos = st.input.len();
                st.status = "Transcribed — press Enter to send.".to_string();
                st.push_event(format!("[voice] {}", &t[..t.len().min(80)]));
            }
            Ok(_) => {
                st.status = "Voice: no speech detected.".to_string();
            }
            Err(e) => {
                st.push_event(format!("[voice] error: {e}"));
                st.status = format!("Voice error: {e}");
            }
        }
    });
}

pub(crate) fn approve_confirm(state: &Arc<Mutex<AppState>>, pc: PendingConfirm) {
    finish_confirm(state, pc, ConfirmResult::Approve, "approved");
}

pub(crate) fn finish_confirm(
    state: &Arc<Mutex<AppState>>,
    pc: PendingConfirm,
    result: ConfirmResult,
    label: &str,
) {
    let tool = pc.tool_name.clone();
    let _ = pc.reply.send(result);
    let mut st = state.lock();
    let first_arg = pc.preview.lines().next().unwrap_or("").to_string();
    let key = (tool.clone(), first_arg.clone());
    let count = st.approval_counts.entry(key).or_insert(0);
    *count += 1;
    if *count == 3 {
        st.push_event(format!(
            "[trust] Approved 3x. Run: harness trust {tool} \"{first_arg}\""
        ));
    }
    st.push_event(format!("[plan] {label}: {tool}"));
    st.status = "Approved — continuing…".to_string();
}

pub(crate) fn handle_search_key(
    state: &Arc<Mutex<AppState>>,
    key: crossterm::event::KeyEvent,
) -> bool {
    let code = key.code;
    let mods = key.modifiers;
    match (code, mods) {
        (KeyCode::Esc, _) | (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
            let mut st = state.lock();
            st.search_mode = false;
            st.search_query.clear();
            st.search_matches.clear();
            st.status = "Ready".to_string();
            true
        }
        (KeyCode::Enter, _) => {
            let mut st = state.lock();
            let nmatches = st.search_matches.len();
            if nmatches > 0 {
                st.search_match_pos = (st.search_match_pos + 1) % nmatches;
                let msg_idx = st.search_matches[st.search_match_pos];
                st.chat_scroll.select(Some(msg_idx));
                st.status = format!(
                    "Search: \"{}\" ({}/{})",
                    st.search_query,
                    st.search_match_pos + 1,
                    nmatches
                );
            }
            true
        }
        (KeyCode::Char('n'), KeyModifiers::NONE) => {
            let mut st = state.lock();
            let nmatches = st.search_matches.len();
            if nmatches > 0 {
                st.search_match_pos = (st.search_match_pos + 1) % nmatches;
                let msg_idx = st.search_matches[st.search_match_pos];
                st.chat_scroll.select(Some(msg_idx));
                st.status = format!(
                    "Search: \"{}\" ({}/{})",
                    st.search_query,
                    st.search_match_pos + 1,
                    nmatches
                );
            }
            true
        }
        (KeyCode::Char('p'), KeyModifiers::NONE) => {
            let mut st = state.lock();
            let nmatches = st.search_matches.len();
            if nmatches > 0 {
                st.search_match_pos = (st.search_match_pos + nmatches - 1) % nmatches;
                let msg_idx = st.search_matches[st.search_match_pos];
                st.chat_scroll.select(Some(msg_idx));
                st.status = format!(
                    "Search: \"{}\" ({}/{})",
                    st.search_query,
                    st.search_match_pos + 1,
                    nmatches
                );
            }
            true
        }
        (KeyCode::Backspace, _) => {
            let mut st = state.lock();
            st.search_query.pop();
            run_search(&mut st);
            true
        }
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
            let mut st = state.lock();
            st.search_query.push(c);
            run_search(&mut st);
            true
        }
        _ => false,
    }
}

fn run_search(st: &mut AppState) {
    let q = st.search_query.to_lowercase();
    st.search_matches = st
        .chat
        .iter()
        .enumerate()
        .filter(|(_, m)| m.content.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect();
    st.search_match_pos = 0;
    let nmatches = st.search_matches.len();
    if let Some(&first) = st.search_matches.first() {
        st.chat_scroll.select(Some(first));
    }
    if q.is_empty() {
        st.status = "Search: ".to_string();
    } else {
        st.status = format!(
            "Search: \"{}\" — {nmatches} match{}",
            q,
            if nmatches == 1 { "" } else { "es" }
        );
    }
}

pub(crate) fn show_help(state: &Arc<Mutex<AppState>>) {
    let mut st = state.lock();
    for line in &[
        "━━━ HARNESS COMMANDS ━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        "SLASH COMMANDS",
        " /clear            clear chat panel",
        " /undo             restore last checkpoint",
        " /diff             show git diff",
        " /test             run test suite",
        " /compact          compact context",
        " /cost             show cost/token breakdown",
        " /plan             plan mode info (use --plan at startup)",
        " /model            show live model (`harness models --set` + restart)",
        " /think [N]        thinking budget (off = adaptive)",
        " /remember t: f    store fact under topic t",
        " /forget t         delete memory topic",
        " /memories         list all topics",
        " /sessions         browse previous sessions",
        " /pr [N]           list PRs / load PR #N context",
        " /issues           list GitHub issues",
        " /ci               show CI runs",
        " /focus [N]        silence notifications N minutes",
        " /ts               toggle timestamps",
        " /notify test      test desktop notification",
        " /trace [last|list] show last turn / list traces",
        " /schema           set structured output JSON schema",
        " /obsidian save    save response to Obsidian",
        " /swarm [gc|refresh]  toggle swarm panel (F2)",
        " /help  F1         this list",
        "KEYBINDINGS",
        " Enter             send message",
        " Shift+Enter       new line in input",
        " ↑/↓              scroll chat (or input history when empty)",
        " PgUp/PgDn         scroll event log / swarm panel",
        " F2                toggle swarm panel",
        " Ctrl+S            voice record (5s, Whisper transcription)",
        " Ctrl+F            search chat",
        " Ctrl+Y            copy last response to clipboard",
        " Ctrl+E            fork session at past turn",
        " Ctrl+[ / Ctrl+]   resize panels",
        " Ctrl+L            scroll to bottom",
        " Ctrl+A/E          line start/end",
        " Ctrl+W            kill word back",
        " Ctrl+U            kill line",
        " Ctrl+K            kill to end",
        " Alt+Left/Right    word jump",
        " Tab               @file autocomplete / slash autocomplete",
        " y / n / a         plan overlay: approve hunk / deny / always-allow",
        " [ / ]             diff review: previous / next hunk",
        " Ctrl+C            quit",
    ] {
        st.push_event(line.to_string());
    }
    st.status = "Help listed in event log →".to_string();
}

// ── Slash command dispatcher ───────────────────────────────────────────────────

pub(crate) async fn handle_slash_command(
    cmd: &str,
    state: &Arc<Mutex<AppState>>,
    session: &mut Session,
    provider: &ArcProvider,
    session_store: &SessionStore,
    agent_tx: &mpsc::Sender<AgentEvent>,
) {
    let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
    let command = parts[0];

    match command {
        "/clear" => {
            let mut st = state.lock();
            st.chat.clear();
            st.event_log.clear();
            st.streaming.clear();
            st.status = "Chat cleared.".to_string();
        }

        "/undo" => {
            let mut st = state.lock();
            match checkpoint::undo() {
                Ok(msg) => {
                    st.push_event(format!("[undo] {msg}"));
                    st.status = "Undo complete.".to_string();
                }
                Err(e) => {
                    st.push_event(format!("[undo] {e}"));
                    st.status = format!("Undo failed: {e}");
                }
            }
        }

        "/diff" => {
            state.lock().push_event("[diff] running git diff…");
            match tokio::process::Command::new("git")
                .args(["diff", "--stat", "HEAD"])
                .output()
                .await
            {
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let mut st = state.lock();
                    for line in text.lines().take(40) {
                        st.push_event(format!("  {line}"));
                    }
                    if text.trim().is_empty() {
                        st.push_event("  (no changes)");
                    }
                    st.status = "git diff in event log.".to_string();
                }
                Err(e) => {
                    state.lock().push_event(format!("[diff] {e}"));
                }
            }
        }

        "/test" => {
            let busy = state.lock().busy;
            if busy {
                state.lock().push_event("[test] agent running.");
                return;
            }
            let test_cmd = detect_test_command();
            {
                let mut st = state.lock();
                st.busy = true;
                st.status = format!("Running: {test_cmd}…");
                st.push_event(format!("[test] {test_cmd}"));
            }
            let atx = agent_tx.clone();
            let state2 = state.clone();
            let cmd_str = test_cmd.clone();
            tokio::spawn(async move {
                let out = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd_str)
                    .output()
                    .await;
                let mut st = state2.lock();
                st.busy = false;
                match out {
                    Ok(o) => {
                        let all = format!(
                            "{}{}",
                            String::from_utf8_lossy(&o.stdout),
                            String::from_utf8_lossy(&o.stderr)
                        );
                        for line in all.lines().take(60) {
                            st.push_event(format!("  {line}"));
                        }
                        let status = if o.status.success() {
                            "passed ✓"
                        } else {
                            "FAILED ✗"
                        };
                        st.status = format!("Tests {status}.");
                        try_emit(
                            Some(&atx),
                            AgentEvent::ToolResult {
                                name: "test".into(),
                                id: "test".into(),
                                result: all,
                            },
                        );
                    }
                    Err(e) => {
                        st.push_event(format!("[test] {e}"));
                        st.status = format!("Test error: {e}");
                    }
                }
            });
        }

        "/cost" => {
            let st = state.lock();
            let (in_tok, out_tok, model_name) = (st.tokens_in, st.tokens_out, st.model.clone());
            drop(st);
            let cost_line = match cost::price_for_model(&model_name) {
                Some(p) => format!(
                    "Cost: {} (↑{} ↓{} @ {})",
                    cost::format_cost(p.cost_usd(in_tok, out_tok)),
                    cost::format_tokens(in_tok),
                    cost::format_tokens(out_tok),
                    model_name
                ),
                None => format!(
                    "Tokens: ↑{} ↓{} (no pricing for {model_name})",
                    cost::format_tokens(in_tok),
                    cost::format_tokens(out_tok)
                ),
            };
            let mut st = state.lock();
            st.push_event(cost_line.clone());
            st.status = cost_line;
        }

        "/plan" => {
            let mut st = state.lock();
            if st.plan_mode {
                st.status = "Plan mode active (confirm gate enabled at startup).".to_string();
            } else {
                st.status =
                    "Plan mode requires `harness --plan` or `[approval] mode = \"plan\"` at startup."
                        .to_string();
            }
        }

        "/model" => {
            let name = parts.get(1).copied().unwrap_or("");
            let live = provider.model().to_string();
            let mut st = state.lock();
            if name.is_empty() {
                st.push_event(format!("[model] live: {live}"));
            } else {
                st.push_event(format!(
                    "[model] cannot switch to `{name}` in-session — run `harness models --set` and restart"
                ));
            }
        }

        "/runs" => match background::list(10) {
            Ok(runs) if runs.is_empty() => state
                .lock()
                .push_event("[runs] No background runs. Use `harness run-bg <prompt>`."),
            Ok(runs) => {
                let mut st = state.lock();
                st.push_event(format!("[runs] {} run(s):", runs.len()));
                for run in &runs {
                    let p = if run.prompt.len() > 50 {
                        format!("{}…", &run.prompt[..50])
                    } else {
                        run.prompt.clone()
                    };
                    st.push_event(format!("  {} [{}] {}", run.id, run.status, p));
                }
            }
            Err(e) => state.lock().push_event(format!("[runs] {e}")),
        },

        "/sessions" => match session_store.list(20) {
            Ok(sessions) if sessions.is_empty() => {
                state.lock().push_event("[sessions] No sessions yet.");
            }
            Ok(sessions) => {
                let mut st = state.lock();
                st.push_event(format!(
                    "[sessions] {} session(s) — use `harness --resume <id>` to load:",
                    sessions.len()
                ));
                for (id, name, updated) in &sessions {
                    let short = &id[..8.min(id.len())];
                    let n = session_store.display_name_for(id, name.clone());
                    st.push_event(format!("  {short}  {n}  {updated}"));
                }
                st.status = format!("{} sessions in event log →", sessions.len());
            }
            Err(e) => state.lock().push_event(format!("[sessions] {e}")),
        },

        "/compact" => {
            let busy = state.lock().busy;
            if busy {
                state.lock().push_event("[compact] agent running.");
                return;
            }
            state.lock().push_event("[compact] compacting…");
            agent::compact_context(provider, session).await;
            let remaining = session.messages.len();
            let mut st = state.lock();
            st.push_event(format!("[compact] {remaining} messages remain."));
            st.status = format!("Compacted ({remaining} messages).");
        }

        "/fork" => {
            state
                .lock()
                .push_event("[fork] Use Ctrl+E to enter fork mode.");
        }

        "/ts" => {
            let mut st = state.lock();
            st.timestamps_visible = !st.timestamps_visible;
            st.status = if st.timestamps_visible {
                "Timestamps ON".into()
            } else {
                "Timestamps OFF".into()
            };
        }

        "/think" => {
            let mut st = state.lock();
            let arg = parts.get(1).copied().unwrap_or("").trim();
            if arg.is_empty() || arg == "off" {
                st.thinking_budget = None;
                st.push_event("[think] OFF — adaptive.");
                st.status = "Thinking: adaptive".to_string();
            } else if let Ok(b) = arg.parse::<u32>() {
                st.thinking_budget = Some(b);
                st.push_event(format!("[think] ON — budget: {b} tokens"));
                st.status = format!("Thinking: {b} tokens");
            } else {
                st.push_event("[think] Usage: /think [N | off]");
            }
        }

        "/focus" => {
            let mut st = state.lock();
            let arg = parts.get(1).copied().unwrap_or("").trim();
            if arg == "off" {
                st.focus_until = None;
                st.status = "Focus mode OFF.".to_string();
            } else {
                let mins: u64 = arg.parse().unwrap_or(25);
                st.focus_until = Some(Instant::now() + Duration::from_secs(mins * 60));
                st.push_event(format!("[focus] {mins}min focus — notifications silenced."));
                st.status = format!("Focus: {mins}min");
            }
        }

        "/remember" => {
            let rest = parts[1..].join(" ");
            if let Some((topic, fact)) = rest.split_once(':') {
                match memory_project::remember(topic.trim(), fact.trim()) {
                    Ok(path) => {
                        let mut st = state.lock();
                        st.push_event(format!("[memory] saved → {}", path.display()));
                        st.status = format!("Remembered under '{}'", topic.trim());
                    }
                    Err(e) => state.lock().push_event(format!("[memory] error: {e}")),
                }
            } else {
                state
                    .lock()
                    .push_event("[memory] Usage: /remember <topic>: <fact>");
            }
        }

        "/forget" => {
            let topic = parts.get(1).copied().unwrap_or("").trim();
            if topic.is_empty() {
                state.lock().push_event("[memory] Usage: /forget <topic>");
            } else {
                match memory_project::forget(topic) {
                    Ok(true) => {
                        let mut st = state.lock();
                        st.push_event(format!("[memory] forgot '{topic}'"));
                        st.status = format!("Forgot '{topic}'");
                    }
                    Ok(false) => state
                        .lock()
                        .push_event(format!("[memory] no memory for '{topic}'")),
                    Err(e) => state.lock().push_event(format!("[memory] {e}")),
                }
            }
        }

        "/memories" => {
            let topics = memory_project::list_topics();
            let mut st = state.lock();
            if topics.is_empty() {
                st.push_event("[memory] no topics. Use /remember topic: fact");
            } else {
                st.push_event(format!("[memory] {} topic(s):", topics.len()));
                for t in &topics {
                    st.push_event(format!("  • {t}"));
                }
            }
            st.status = format!("{} topics", topics.len());
        }

        "/pr" => {
            let pr_num = parts.get(1).copied().unwrap_or("").trim();
            if pr_num.is_empty() {
                state.lock().push_event("[pr] fetching PRs…");
                let state2 = state.clone();
                tokio::spawn(async move {
                    let msg = harness_tools::tools::gh::pr_list()
                        .await
                        .unwrap_or_else(|e| format!("gh error: {e}"));
                    let mut st = state2.lock();
                    for line in msg.lines().take(30) {
                        st.push_event(format!("  {line}"));
                    }
                    st.status = "PRs in event log →".to_string();
                });
            } else {
                let num = pr_num.to_string();
                let mut st = state.lock();
                st.input =
                    format!("Review PR #{num} — fetch diff, comments, and CI status. Summarize and suggest improvements.");
                st.cursor_pos = st.input.len();
                st.status = format!("PR #{num} loaded — press Enter to review");
            }
        }

        "/issues" => {
            state.lock().push_event("[issues] fetching…");
            let state2 = state.clone();
            tokio::spawn(async move {
                let out = tokio::process::Command::new("gh")
                    .args(["issue", "list", "--limit", "20"])
                    .output()
                    .await;
                let msg = match out {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
                    Err(e) => format!("gh error: {e}"),
                };
                let mut st = state2.lock();
                for line in msg.lines().take(40) {
                    st.push_event(format!("  {line}"));
                }
                st.status = "Issues in event log →".to_string();
            });
        }

        "/ci" => {
            state.lock().push_event("[ci] checking runs…");
            let state2 = state.clone();
            let notif_cfg = state.lock().notifications.clone();
            tokio::spawn(async move {
                let out = tokio::process::Command::new("gh")
                    .args(["run", "list", "--limit", "10"])
                    .output()
                    .await;
                let msg = match out {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
                    Err(e) => format!("gh error: {e}"),
                };
                let mut st = state2.lock();
                for line in msg.lines().take(20) {
                    st.push_event(format!("  {line}"));
                    let lower = line.to_lowercase();
                    if lower.contains("failure") || lower.contains("failed") {
                        notifications::ci_failed(&notif_cfg, line.trim(), "");
                    }
                }
                st.status = "CI runs in event log →".to_string();
            });
        }

        "/notify" => {
            let notif_cfg = state.lock().notifications.clone();
            notifications::test_notification(&notif_cfg);
            state.lock().push_event("[notify] test notification sent");
        }

        "/obsidian" => {
            let rest = cmd.trim_start_matches("/obsidian").trim();
            match rest {
                "" | "help" | "?" => {
                    let mut st = state.lock();
                    st.push_event("[obsidian] usage:");
                    st.push_event("  /obsidian save   save last assistant reply to Obsidian");
                    st.push_event("  /obsidian help   this message");
                    st.push_event(
                        "Enable in config (project .harness/config.toml or ~/.harness/config.toml):",
                    );
                    st.push_event("  [bridges.obsidian]");
                    st.push_event("  enabled = true");
                    st.push_event("  vault = \"MyVault\"    # optional vault name");
                    st.push_event("  folder = \"Harness\"   # optional folder (default Harness)");
                    st.push_event(
                        "See config/default.toml [bridges.obsidian] and `harness bridge obsidian`.",
                    );
                    st.status = "Obsidian help in event log →".into();
                }
                "save" => {
                    let (content, obs_cfg) = {
                        let st = state.lock();
                        let content = st
                            .chat
                            .iter()
                            .rev()
                            .find(|m| m.role == "assistant")
                            .map(|m| m.content.clone());
                        (content, st.obsidian.clone())
                    };
                    let Some(content) = content else {
                        state
                            .lock()
                            .push_event("[obsidian] no assistant response to save yet.");
                        return;
                    };
                    if !obs_cfg.enabled {
                        let mut st = state.lock();
                        st.push_event(
                            "[obsidian] bridge not enabled. Add to config (see config/default.toml):",
                        );
                        st.push_event("  [bridges.obsidian]");
                        st.push_event("  enabled = true");
                        st.push_event("  vault = \"MyVault\"    # optional");
                        st.push_event("  folder = \"Harness\"   # optional");
                        st.push_event(
                            "Then restart harness (or `harness bridge obsidian <title> <content>`).",
                        );
                        st.status = "Obsidian disabled — see event log".into();
                        return;
                    }
                    // Prefer a short title from the first non-empty line; fallback to timestamp.
                    let title = content
                        .lines()
                        .map(str::trim)
                        .find(|l| !l.is_empty())
                        .map(|l| {
                            let t = l.trim_start_matches('#').trim();
                            let t = if t.chars().count() > 60 {
                                format!("{}…", t.chars().take(57).collect::<String>())
                            } else {
                                t.to_string()
                            };
                            // Obsidian path-safe-ish title
                            t.replace(['/', '\\', ':'], "-")
                        })
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| {
                            chrono::Local::now()
                                .format("Harness %Y-%m-%d %H%M")
                                .to_string()
                        });
                    let state2 = state.clone();
                    let title_log = title.clone();
                    tokio::spawn(async move {
                        match crate::bridges::obsidian_write(&obs_cfg, &title, &content).await {
                            Ok(()) => {
                                let mut st = state2.lock();
                                st.push_event(format!("[obsidian] note queued: {title_log}"));
                                st.status = format!("Obsidian: {title_log}");
                            }
                            Err(e) => {
                                let mut st = state2.lock();
                                st.push_event(format!("[obsidian] failed: {e}"));
                                st.status = "Obsidian save failed".into();
                            }
                        }
                    });
                }
                other => {
                    state.lock().push_event(format!(
                        "[obsidian] unknown subcommand `{other}` — try /obsidian save or /obsidian help"
                    ));
                }
            }
        }

        "/trace" => {
            let rest = cmd.trim_start_matches("/trace").trim();
            match rest {
                "" | "last" => match crate::observability::load_last_trace() {
                    Ok(spans) if spans.is_empty() => {
                        let mut st = state.lock();
                        st.push_event("[trace] no traces found under ~/.harness/traces/.");
                        st.push_event("[trace] enable local traces in config:");
                        st.push_event("  [observability]");
                        st.push_event("  enabled = true");
                        st.push_event("  local_traces = true");
                        st.push_event(
                            "See config/default.toml [observability] or `harness trace`.",
                        );
                        st.status = "No traces".into();
                    }
                    Ok(spans) => {
                        let mut st = state.lock();
                        let tid = &spans[0].trace_id;
                        st.push_event(format!(
                            "[trace] {tid} — {} span{}",
                            spans.len(),
                            if spans.len() == 1 { "" } else { "s" }
                        ));
                        for s in spans.iter().take(40) {
                            let status = match &s.status {
                                crate::observability::SpanStatus::Ok => "ok".to_string(),
                                crate::observability::SpanStatus::Error(e) => {
                                    format!("err:{e}")
                                }
                            };
                            st.push_event(format!(
                                "  {:<36} {:>6}ms  {status}",
                                s.name, s.duration_ms
                            ));
                        }
                        if spans.len() > 40 {
                            st.push_event(format!(
                                "  … {} more (use `harness trace {tid}`)",
                                spans.len() - 40
                            ));
                        }
                        st.status = format!("Trace {tid}");
                    }
                    Err(e) => {
                        state
                            .lock()
                            .push_event(format!("[trace] failed to load: {e}"));
                    }
                },
                "list" => match crate::observability::list_traces(15) {
                    Ok(files) if files.is_empty() => {
                        let mut st = state.lock();
                        st.push_event("[trace] no traces found under ~/.harness/traces/.");
                        st.push_event(
                                "[trace] enable [observability] local_traces = true (config/default.toml).",
                            );
                        st.status = "No traces".into();
                    }
                    Ok(files) => {
                        let mut st = state.lock();
                        st.push_event(format!(
                            "[trace] {} recent file{} in ~/.harness/traces/:",
                            files.len(),
                            if files.len() == 1 { "" } else { "s" }
                        ));
                        for p in &files {
                            let name = std::path::Path::new(p)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or(p);
                            st.push_event(format!("  {name}"));
                        }
                        st.push_event("[trace] /trace last — detail last; CLI: harness trace <id>");
                        st.status = "Trace list in event log →".into();
                    }
                    Err(e) => {
                        state.lock().push_event(format!("[trace] list failed: {e}"));
                    }
                },
                "help" | "?" => {
                    let mut st = state.lock();
                    st.push_event("[trace] usage:");
                    st.push_event("  /trace        show last turn trace (same as /trace last)");
                    st.push_event("  /trace last   show last turn trace");
                    st.push_event("  /trace list   list recent trace ids");
                    st.push_event("CLI: harness trace | harness trace <id>");
                    st.push_event("Config: [observability] in config/default.toml");
                    st.status = "Trace help in event log →".into();
                }
                other => {
                    // Treat bare id-like args as "show this trace" via load of that file.
                    let dir = dirs::home_dir().unwrap_or_default().join(".harness/traces");
                    let file = dir.join(format!("{other}.jsonl"));
                    if file.exists() {
                        match std::fs::read_to_string(&file) {
                            Ok(text) => {
                                let spans: Vec<crate::observability::Span> = text
                                    .lines()
                                    .filter(|l| !l.trim().is_empty())
                                    .filter_map(|l| serde_json::from_str(l).ok())
                                    .collect();
                                let mut st = state.lock();
                                if spans.is_empty() {
                                    st.push_event(format!(
                                        "[trace] {other}: file empty or unreadable spans"
                                    ));
                                } else {
                                    st.push_event(format!(
                                        "[trace] {other} — {} span{}",
                                        spans.len(),
                                        if spans.len() == 1 { "" } else { "s" }
                                    ));
                                    for s in spans.iter().take(40) {
                                        st.push_event(format!(
                                            "  {:<36} {:>6}ms",
                                            s.name, s.duration_ms
                                        ));
                                    }
                                }
                                st.status = format!("Trace {other}");
                            }
                            Err(e) => {
                                state.lock().push_event(format!("[trace] read failed: {e}"));
                            }
                        }
                    } else {
                        state.lock().push_event(format!(
                            "[trace] unknown `{other}` — try /trace, /trace last, /trace list, or /trace help"
                        ));
                    }
                }
            }
        }

        "/schema" => {
            let rest = cmd.trim_start_matches("/schema").trim();
            if rest == "clear" || rest.is_empty() {
                state.lock().response_schema = None;
                state
                    .lock()
                    .push_event("[schema] structured output cleared.");
            } else {
                let mut schema_parts = rest.splitn(2, ' ');
                let name = schema_parts.next().unwrap_or("response");
                let schema_str = schema_parts.next().unwrap_or("{}");
                match serde_json::from_str::<serde_json::Value>(schema_str) {
                    Ok(schema_val) => {
                        let rs = harness_provider_core::ResponseSchema::new(name, schema_val);
                        let msg = format!(
                            "[schema] set to '{}' — responses will be strict JSON.",
                            rs.name
                        );
                        state.lock().response_schema = Some(rs);
                        state.lock().push_event(msg);
                    }
                    Err(e) => {
                        state
                            .lock()
                            .push_event(format!("[schema] invalid JSON: {e}"));
                    }
                }
            }
        }

        "/swarm" => {
            let rest = cmd.trim_start_matches("/swarm").trim();
            match rest {
                "" | "toggle" | "panel" => {
                    state.lock().toggle_swarm_panel();
                }
                "refresh" => {
                    let mut st = state.lock();
                    st.refresh_swarm();
                    if st.right_panel_mode != super::state::RightPanelMode::Swarm {
                        st.right_panel_mode = super::state::RightPanelMode::Swarm;
                    }
                    st.status = "Swarm panel refreshed".into();
                }
                s if s == "gc" || s.starts_with("gc ") => {
                    let mut stale_secs = 3600u64;
                    let mut keep: Option<usize> = None;
                    for tok in s.split_whitespace().skip(1) {
                        if let Some(v) = tok.strip_prefix("stale=") {
                            if let Ok(n) = v.parse() {
                                stale_secs = n;
                            }
                        } else if let Some(v) = tok.strip_prefix("keep=") {
                            if let Ok(n) = v.parse() {
                                keep = Some(n);
                            }
                        } else if let Ok(n) = tok.parse::<u64>() {
                            stale_secs = n;
                        }
                    }
                    match crate::swarm::gc(&crate::swarm::GcOptions {
                        stale_secs,
                        keep_terminal: keep,
                        older_than_secs: None,
                        dry_run: false,
                    }) {
                        Ok(report) => {
                            let mut st = state.lock();
                            st.push_event(format!("[swarm] {}", report.summary()));
                            for (id, reason) in report.reaped.iter().take(8) {
                                st.push_event(format!("[swarm] reaped {id}: {reason}"));
                            }
                            st.refresh_swarm();
                            st.right_panel_mode = super::state::RightPanelMode::Swarm;
                            st.status = format!("[swarm] {}", report.summary());
                        }
                        Err(e) => {
                            state.lock().push_event(format!("[swarm] gc failed: {e}"));
                        }
                    }
                }
                other => {
                    state.lock().push_event(format!(
                        "[swarm] unknown subcommand `{other}` — try /swarm, /swarm refresh, /swarm gc"
                    ));
                }
            }
        }

        "/help" | "/?" => {
            show_help(state);
        }

        _ => {
            state
                .lock()
                .push_event(format!("[unknown] {cmd} — type /help or press F1"));
        }
    }
}
