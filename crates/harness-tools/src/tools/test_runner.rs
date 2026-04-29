//! `test_runner` — detect and run tests for any project type.
//!
//! Returns a structured summary instead of raw output, so the agent can
//! immediately understand what passed/failed and self-correct.

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};

use crate::registry::Tool;

pub struct TestRunnerTool;

/// Structured test result returned to the agent.
#[derive(Debug)]
pub struct TestReport {
    pub passed: usize,
    pub failed: usize,
    pub errors: Vec<TestFailure>,
    pub raw_output: String,
}

#[derive(Debug)]
pub struct TestFailure {
    pub name: String,
    pub message: String,
}

impl TestReport {
    pub fn to_agent_string(&self) -> String {
        let status = if self.failed == 0 { "PASS" } else { "FAIL" };
        let mut out = format!("[{status}] {} passed, {} failed\n", self.passed, self.failed);
        for f in &self.errors {
            out.push_str(&format!("  FAILED: {}\n    {}\n", f.name, f.message));
        }
        if self.failed == 0 {
            out.push_str("All tests passed.");
        }
        out
    }
}

#[async_trait]
impl Tool for TestRunnerTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "test_runner",
            "Run the test suite for the current project and return a structured summary. \
             Auto-detects Rust (cargo test), Node.js (npm test / vitest), \
             Python (pytest), and Go (go test). \
             Optionally scope to a specific package or file.",
            json!({
                "type": "object",
                "properties": {
                    "scope": {
                        "type": "string",
                        "description": "Optional: package name, file path, or test filter to run a subset."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Max run time in seconds (default 120)."
                    }
                }
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let scope = args["scope"].as_str();
        let timeout = args["timeout_secs"].as_u64().unwrap_or(120);

        let (cmd, runner) = detect_test_command(scope);

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("test run timed out after {timeout}s"))??;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let raw = format!("{stdout}{stderr}");

        let report = parse_output(&runner, &raw, output.status.success());
        Ok(report.to_agent_string())
    }
}

#[derive(Debug, PartialEq)]
enum Runner {
    Cargo,
    Npm,
    Pytest,
    Go,
    Make,
}

fn detect_test_command(scope: Option<&str>) -> (String, Runner) {
    if std::path::Path::new("Cargo.toml").exists() {
        let cmd = match scope {
            Some(s) if s.contains('/') => format!("cargo test --package {} 2>&1", s.split('/').next().unwrap_or(s)),
            Some(s) => format!("cargo test {} 2>&1", s),
            None => "cargo test 2>&1".to_string(),
        };
        return (cmd, Runner::Cargo);
    }

    if std::path::Path::new("package.json").exists() {
        let cmd = match scope {
            Some(s) => format!("npm test -- {s} 2>&1"),
            None => "npm test 2>&1".to_string(),
        };
        return (cmd, Runner::Npm);
    }

    if std::path::Path::new("pyproject.toml").exists()
        || std::path::Path::new("setup.py").exists()
    {
        let cmd = match scope {
            Some(s) => format!("python -m pytest {s} -v 2>&1"),
            None => "python -m pytest -v 2>&1".to_string(),
        };
        return (cmd, Runner::Pytest);
    }

    if std::path::Path::new("go.mod").exists() {
        let cmd = match scope {
            Some(s) => format!("go test {s} 2>&1"),
            None => "go test ./... 2>&1".to_string(),
        };
        return (cmd, Runner::Go);
    }

    ("make test 2>&1".to_string(), Runner::Make)
}

fn parse_output(runner: &Runner, output: &str, success: bool) -> TestReport {
    match runner {
        Runner::Cargo => parse_cargo(output, success),
        Runner::Pytest => parse_pytest(output, success),
        Runner::Go => parse_go(output, success),
        Runner::Npm | Runner::Make => parse_generic(output, success),
    }
}

fn parse_cargo(output: &str, _success: bool) -> TestReport {
    let mut passed = 0;
    let mut failed = 0;
    let mut errors: Vec<TestFailure> = Vec::new();

    for line in output.lines() {
        if line.starts_with("test ") && line.ends_with(" ... ok") {
            passed += 1;
        } else if line.starts_with("test ") && line.contains("FAILED") {
            failed += 1;
            let name = line.trim_start_matches("test ").split(" ...").next().unwrap_or("?").to_string();
            errors.push(TestFailure { name, message: "test failed".to_string() });
        } else if line.contains("test result:") {
            // e.g. "test result: FAILED. 3 passed; 2 failed; ..."
            if let Some(p) = extract_number(line, "passed") { passed = p; }
            if let Some(f) = extract_number(line, "failed") { failed = f; }
        }
    }

    // Extract FAILED: messages.
    let mut in_failure = false;
    let mut current_name = String::new();
    let mut current_msg = String::new();

    for line in output.lines() {
        if line.starts_with("failures:") {
            in_failure = true;
        } else if in_failure {
            if line.starts_with("---- ") && line.ends_with(" stdout ----") {
                if !current_name.is_empty() && !current_msg.is_empty() {
                    if let Some(e) = errors.iter_mut().find(|e| e.name == current_name) {
                        e.message = current_msg.trim().to_string();
                    }
                }
                current_name = line.trim_start_matches("---- ").split(' ').next().unwrap_or("?").to_string();
                current_msg = String::new();
            } else if !line.is_empty() && !line.starts_with("failures:") {
                current_msg.push_str(line);
                current_msg.push('\n');
            }
        }
    }

    TestReport { passed, failed, errors, raw_output: output.to_string() }
}

fn parse_pytest(output: &str, success: bool) -> TestReport {
    let mut passed = 0;
    let mut failed = 0;
    let mut errors: Vec<TestFailure> = Vec::new();

    for line in output.lines() {
        // "== 3 passed, 1 failed in 0.12s =="
        if line.contains(" passed") || line.contains(" failed") {
            if let Some(p) = extract_number(line, "passed") { passed = p; }
            if let Some(f) = extract_number(line, "failed") { failed = f; }
        }
        // "FAILED test_file.py::test_name - AssertionError"
        if line.starts_with("FAILED ") {
            let rest = line.trim_start_matches("FAILED ");
            let (name, msg) = rest.split_once(" - ").unwrap_or((rest, "failed"));
            errors.push(TestFailure { name: name.to_string(), message: msg.to_string() });
        }
    }

    if passed == 0 && failed == 0 && !success {
        failed = 1;
    }

    TestReport { passed, failed, errors, raw_output: output.to_string() }
}

fn parse_go(output: &str, _success: bool) -> TestReport {
    let mut passed = 0;
    let mut failed = 0;
    let mut errors: Vec<TestFailure> = Vec::new();

    for line in output.lines() {
        if line.starts_with("--- PASS:") {
            passed += 1;
        } else if line.starts_with("--- FAIL:") {
            failed += 1;
            let name = line.trim_start_matches("--- FAIL: ").split(' ').next().unwrap_or("?").to_string();
            errors.push(TestFailure { name, message: "test failed".to_string() });
        }
    }

    TestReport { passed, failed, errors, raw_output: output.to_string() }
}

fn parse_generic(output: &str, success: bool) -> TestReport {
    let failed = if success { 0 } else { 1 };
    TestReport {
        passed: 0,
        failed,
        errors: if failed > 0 {
            vec![TestFailure { name: "test".into(), message: output.lines().last().unwrap_or("failed").to_string() }]
        } else {
            vec![]
        },
        raw_output: output.to_string(),
    }
}

fn extract_number(line: &str, word: &str) -> Option<usize> {
    let idx = line.find(word)?;
    let before = line[..idx].trim_end();
    before.split_whitespace().last()?.parse().ok()
}
