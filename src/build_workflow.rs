//! A durable, editable build brief shared by CLI, terminal, and HTTP sessions.

use crate::work_skills::WorkKind;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const BRIEF_PATH: &str = ".harness/build.toml";
pub const BUILD_RULES: &str = "Build a working, verifiable slice of the requested system. Inspect the repository and preserve unrelated edits before changing it. Use its existing stack and conventions. Establish input/output contracts and failure behavior before expanding scope. For a backend, verify startup and a real request as well as tests. For RAG, retain source identifiers, ground citations in retrieved records, and handle empty retrieval honestly. For automation, address retries, duplicate execution, cancellation, and recovery where relevant. Distinguish synthetic checks from live provider behavior. Run the brief's verification commands and fix failures; if a check cannot run, report it as unverified with the actual blocker. Never infer deployment or publication from a build. Finish with what works, evidence from checks, an exact local run command, and remaining gates. Keep .harness/BUILD_PROGRESS.md current with decisions, observed checks, and the next useful action so another session can continue from actual work.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildBrief {
    pub version: u32,
    pub goal: String,
    #[serde(default)]
    pub kind: WorkKind,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub checks: Vec<String>,
}

impl BuildBrief {
    pub fn load(root: &Path) -> Result<Option<Self>> {
        let path = root.join(BRIEF_PATH);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        let brief: Self = toml::from_str(&text).context("invalid .harness/build.toml")?;
        brief.validate()?;
        Ok(Some(brief))
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.version == 1,
            "unsupported build brief version {}",
            self.version
        );
        anyhow::ensure!(
            !self.goal.trim().is_empty(),
            "build outcome cannot be empty"
        );
        anyhow::ensure!(
            self.acceptance
                .iter()
                .chain(&self.checks)
                .all(|value| !value.trim().is_empty()),
            "acceptance criteria and checks cannot be empty"
        );
        Ok(())
    }

    pub fn instructions(&self) -> String {
        let mut text = format!(
            "## Active build brief\n\nOutcome: {}\n\nAcceptance criteria:\n",
            self.goal
        );
        for criterion in &self.acceptance {
            text.push_str(&format!("- {criterion}\n"));
        }
        text.push_str("\nVerification commands (not yet evidence of passing):\n");
        if self.checks.is_empty() {
            text.push_str("No commands configured. Inspect the project and establish suitable checks before claiming completion.\n");
        }
        for check in &self.checks {
            text.push_str(&format!("- `{check}`\n"));
        }
        text.push_str(&format!(
            "\nWorkflow: {}\n{}\n",
            self.kind.name(),
            self.kind.rules()
        ));
        text.push_str("Use .harness/BUILD_PROGRESS.md to retain decisions, observed checks, and the next useful action. Report actual evidence and remaining gates; a finished turn is not acceptance.\n");
        text
    }
}

/// Write a new brief, archive replaced outcomes, or reuse the existing brief.
/// Preparation performs no provider calls and runs no verification commands.
pub fn prepare(
    root: &Path,
    goal: Option<&str>,
    acceptance: &[String],
    checks: &[String],
) -> Result<String> {
    prepare_work(root, goal, Some(WorkKind::Software), acceptance, checks)
}

pub fn prepare_work(
    root: &Path,
    goal: Option<&str>,
    kind: Option<WorkKind>,
    acceptance: &[String],
    checks: &[String],
) -> Result<String> {
    let previous = BuildBrief::load(root)?;
    let kind = kind.unwrap_or_else(|| previous.as_ref().map(|b| b.kind).unwrap_or_default());
    let mut brief = match goal {
        Some(goal) => {
            anyhow::ensure!(!goal.trim().is_empty(), "build outcome cannot be empty");
            match &previous {
                Some(old) if old.goal == goal.trim() && old.kind == kind => old.clone(),
                _ => BuildBrief {
                    version: 1,
                    goal: goal.trim().to_string(),
                    kind,
                    acceptance: kind.acceptance().iter().map(|s| s.to_string()).collect(),
                    checks: if kind.is_code() {
                        detected_checks(root)
                    } else {
                        Vec::new()
                    },
                },
            }
        }
        None => previous
            .clone()
            .context("no build brief exists; run harness build \"your desired outcome\"")?,
    };
    if brief.kind != kind {
        anyhow::bail!("changing workflow kind requires an explicit outcome; use harness work --kind {} \"outcome\"", kind.name());
    }
    if !acceptance.is_empty() {
        brief.acceptance = acceptance.to_vec();
    }
    if !checks.is_empty() {
        brief.checks = checks.to_vec();
    }
    brief.validate()?;
    crate::work_skills::install(root, kind)?;
    let directory = root.join(".harness");
    std::fs::create_dir_all(&directory)?;
    let mut archived_progress = false;
    if let Some(old) = &previous {
        if old.goal != brief.goal || old.kind != brief.kind {
            let history = directory.join("build-history");
            std::fs::create_dir_all(&history)?;
            let id = uuid::Uuid::new_v4();
            std::fs::write(
                history.join(format!("{id}.toml")),
                toml::to_string_pretty(old)?,
            )?;
            let progress = directory.join("BUILD_PROGRESS.md");
            if progress.exists() {
                std::fs::copy(&progress, history.join(format!("{id}-progress.md")))?;
                archived_progress = true;
            }
        }
    }
    let tmp = directory.join(format!(".build-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, toml::to_string_pretty(&brief)?)?;
    std::fs::rename(tmp, root.join(BRIEF_PATH))?;
    if archived_progress {
        std::fs::remove_file(directory.join("BUILD_PROGRESS.md"))?;
    }
    println!("Build brief: {BRIEF_PATH}");
    println!("Outcome: {}", brief.goal);
    println!("Workflow: {}", brief.kind.name());
    println!(
        "{} acceptance criteria; {} verification commands",
        brief.acceptance.len(),
        brief.checks.len()
    );
    Ok(format!("Continue the active {} work brief. Inspect actual workspace state, read the matching skill at .agents/skills/{}/SKILL.md, and read .harness/BUILD_PROGRESS.md if it exists. Missing progress means new work; proceed from the brief. Complete and verify the next useful slice toward this outcome: {}", brief.kind.name(), brief.kind.name(), brief.goal))
}

fn detected_checks(root: &Path) -> Vec<String> {
    let mut checks = Vec::new();
    if root.join("Cargo.toml").is_file() {
        checks.extend(["cargo check --locked".into(), "cargo test --locked".into()]);
    }
    if let Ok(package) = std::fs::read_to_string(root.join("package.json")) {
        if let Ok(package) = serde_json::from_str::<serde_json::Value>(&package) {
            let manager = if root.join("pnpm-lock.yaml").exists() {
                "pnpm"
            } else if root.join("yarn.lock").exists() {
                "yarn"
            } else if root.join("bun.lock").exists() || root.join("bun.lockb").exists() {
                "bun"
            } else {
                "npm"
            };
            for script in ["typecheck", "lint", "test", "build"] {
                if package["scripts"][script].is_string() {
                    checks.push(format!("{manager} run {script}"));
                }
            }
        }
    }
    if (root.join("pyproject.toml").is_file() || root.join("setup.py").is_file())
        && (root.join("tests").is_dir() || root.join("pytest.ini").is_file())
    {
        let python = if root.join(".venv/bin/python").is_file() {
            ".venv/bin/python"
        } else if root.join(".venv/Scripts/python.exe").is_file() {
            ".venv/Scripts/python.exe"
        } else {
            "python3"
        };
        checks.push(format!("{python} -m pytest"));
    }
    if root.join("go.mod").is_file() {
        checks.push("go test ./...".into());
    }
    checks
}

/// Persist actual runner observations, without treating turn completion as acceptance.
pub fn record_run(
    root: &Path,
    session_id: &str,
    finished: bool,
    observed_commands: &[(String, String)],
) -> Result<()> {
    let directory = root.join(".harness");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("BUILD_PROGRESS.md");
    let previous = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("reading prior build progress"),
    };
    let status = if finished {
        "finished"
    } else {
        "stopped with an error or cancellation"
    };
    let mut record = format!(
        "# Observed Harness build run — {}\n\nSession: `{session_id}`\nAgent turn: {status}. This is turn status, not certification that the acceptance criteria passed.\n\n## Recent command/tool output excerpts\n\n",
        chrono::Utc::now().to_rfc3339()
    );
    if observed_commands.is_empty() {
        record.push_str("No completed shell or test-runner output was observed in this turn. Verification remains open.\n");
    }
    let start = observed_commands.len().saturating_sub(4);
    if start > 0 {
        record.push_str(&format!("Showing the last four command results; {start} earlier results remain in the saved session.\n\n"));
    }
    for (name, output) in &observed_commands[start..] {
        let excerpt = if output.len() > 3_000 {
            format!(
                "{}\n[... output excerpt omitted ...]\n{}",
                harness_tools::text::byte_prefix(output, 1_500),
                harness_tools::text::byte_suffix(output, 1_500)
            )
        } else {
            output.clone()
        };
        record.push_str(&format!("### {name}\n\n~~~text\n{}\n~~~\n\n", excerpt));
    }
    record.push_str("Inspect the saved session for exact commands and full results. Recheck these observations against the actual workspace before continuing.\n");
    if !previous.trim().is_empty() {
        record.push_str(&format!(
            "\n## Earlier progress and project notes\n\n{previous}"
        ));
    }
    let tmp = directory.join(format!(".progress-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, record)?;
    std::fs::rename(tmp, path).context("saving build progress")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn research_continues_its_kind_and_archives_on_kind_change() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        prepare_work(
            dir.path(),
            Some("Investigate retrieval"),
            Some(WorkKind::Research),
            &[],
            &[],
        )
        .unwrap();
        let original = BuildBrief::load(dir.path()).unwrap().unwrap();
        assert!(original.checks.is_empty());
        std::fs::write(
            dir.path().join(".harness/BUILD_PROGRESS.md"),
            "Evidence so far",
        )
        .unwrap();
        prepare_work(dir.path(), None, None, &[], &[]).unwrap();
        assert_eq!(
            BuildBrief::load(dir.path()).unwrap().unwrap().kind,
            WorkKind::Research
        );
        assert!(prepare_work(dir.path(), None, Some(WorkKind::Software), &[], &[]).is_err());
        prepare_work(
            dir.path(),
            Some("Investigate retrieval"),
            Some(WorkKind::Software),
            &[],
            &[],
        )
        .unwrap();
        assert!(!dir.path().join(".harness/BUILD_PROGRESS.md").exists());
        assert_eq!(
            std::fs::read_dir(dir.path().join(".harness/build-history"))
                .unwrap()
                .count(),
            2
        );
        assert_eq!(
            BuildBrief::load(dir.path()).unwrap().unwrap().checks.len(),
            2
        );
    }

    #[test]
    fn old_brief_without_kind_loads_as_software() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".harness")).unwrap();
        std::fs::write(
            dir.path().join(BRIEF_PATH),
            "version = 1\ngoal = 'Original build'",
        )
        .unwrap();
        assert_eq!(
            BuildBrief::load(dir.path()).unwrap().unwrap().kind,
            WorkKind::Software
        );
        prepare(dir.path(), None, &[], &[]).unwrap();
    }

    #[test]
    fn persists_and_reuses_brief_without_resetting_custom_checks() {
        let dir = tempdir().unwrap();
        prepare(
            dir.path(),
            Some("Build a retrieval API"),
            &["Sources are cited".into()],
            &["python3 -m unittest".into()],
        )
        .unwrap();
        prepare(dir.path(), None, &[], &[]).unwrap();
        let brief = BuildBrief::load(dir.path()).unwrap().unwrap();
        assert_eq!(brief.checks, ["python3 -m unittest"]);
        assert!(brief.instructions().contains("Sources are cited"));
    }

    #[test]
    fn changing_outcome_archives_prior_work_and_rejects_empty_updates() {
        let dir = tempdir().unwrap();
        prepare(dir.path(), Some("First"), &[], &[]).unwrap();
        std::fs::write(
            dir.path().join(".harness/BUILD_PROGRESS.md"),
            "Observed work from First",
        )
        .unwrap();
        assert!(prepare(dir.path(), Some(" "), &[], &[]).is_err());
        assert_eq!(BuildBrief::load(dir.path()).unwrap().unwrap().goal, "First");
        prepare(dir.path(), Some("Second"), &[], &[]).unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path().join(".harness/build-history"))
                .unwrap()
                .count(),
            2
        );
        assert!(!dir.path().join(".harness/BUILD_PROGRESS.md").exists());
    }

    #[test]
    fn detects_existing_commands_without_inventing_tests() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":"vite build","lint":"eslint"}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(
            detected_checks(dir.path()),
            ["pnpm run lint", "pnpm run build"]
        );
        std::fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();
        std::fs::create_dir(dir.path().join("tests")).unwrap();
        assert!(detected_checks(dir.path()).contains(&"python3 -m pytest".into()));
    }

    #[test]
    fn missing_and_invalid_briefs_fail_explicitly() {
        let dir = tempdir().unwrap();
        assert!(prepare(dir.path(), None, &[], &[]).is_err());
        std::fs::create_dir(dir.path().join(".harness")).unwrap();
        std::fs::write(dir.path().join(BRIEF_PATH), "version = 99\ngoal = 'x'").unwrap();
        assert!(BuildBrief::load(dir.path()).is_err());
    }

    #[test]
    fn runner_observations_preserve_notes_without_certifying_acceptance() {
        let dir = tempdir().unwrap();
        prepare(dir.path(), Some("Build retrieval"), &[], &[]).unwrap();
        let path = dir.path().join(".harness/BUILD_PROGRESS.md");
        std::fs::write(&path, "Next: verify empty retrieval").unwrap();
        record_run(
            dir.path(),
            "session-one",
            true,
            &[("shell".into(), "Ran 2 tests\nOK".into())],
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("session-one"));
        assert!(text.contains("Ran 2 tests"));
        assert!(text.contains("Next: verify empty retrieval"));
        assert!(text.contains("not certification"));
        record_run(dir.path(), "session-two", false, &[]).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.find("session-two").unwrap() < text.find("session-one").unwrap());
        assert!(text.contains("stopped with an error"));
        assert!(text.contains("Verification remains open"));
        let long_failure = format!("{}CHECK_FAILED", "é".repeat(2_000));
        record_run(
            dir.path(),
            "session-three",
            false,
            &[("shell".into(), long_failure)],
        )
        .unwrap();
        let text = std::fs::read_to_string(dir.path().join(".harness/BUILD_PROGRESS.md")).unwrap();
        assert!(text.contains("CHECK_FAILED"));
        assert!(text.contains("output excerpt omitted"));
    }
}
