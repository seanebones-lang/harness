//! A durable, editable build brief shared by CLI, terminal, and HTTP sessions.

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
        text.push_str(&format!("\n{BUILD_RULES}\n"));
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
    let previous = BuildBrief::load(root)?;
    let mut brief = match goal {
        Some(goal) => {
            anyhow::ensure!(!goal.trim().is_empty(), "build outcome cannot be empty");
            match &previous {
                Some(old) if old.goal == goal.trim() => old.clone(),
                _ => BuildBrief {
                    version: 1,
                    goal: goal.trim().to_string(),
                    acceptance: vec!["A useful end-to-end path works with documented inputs, outputs, and failure behavior".into(), "Relevant checks and a local smoke example pass, with unverified integrations identified".into(), "README explains setup, configuration, and an exact local run command".into()],
                    checks: detected_checks(root),
                },
            }
        }
        None => previous
            .clone()
            .context("no build brief exists; run harness build \"your desired outcome\"")?,
    };
    if !acceptance.is_empty() {
        brief.acceptance = acceptance.to_vec();
    }
    if !checks.is_empty() {
        brief.checks = checks.to_vec();
    }
    brief.validate()?;
    let directory = root.join(".harness");
    std::fs::create_dir_all(&directory)?;
    let mut archived_progress = false;
    if let Some(old) = &previous {
        if old.goal != brief.goal {
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
    println!(
        "{} acceptance criteria; {} verification commands",
        brief.acceptance.len(),
        brief.checks.len()
    );
    Ok(format!("Continue the active build brief. Inspect actual repository state and read .harness/BUILD_PROGRESS.md if it exists. Missing progress means this is a new build; proceed from the brief. Implement and verify the next useful slice toward this outcome: {}", brief.goal))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
}
