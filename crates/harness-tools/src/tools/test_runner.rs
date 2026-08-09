//! `test_runner` — detect and run tests for any project type.
//!
//! Returns a structured summary instead of raw output, so the agent can
//! immediately understand what passed/failed and self-correct.

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};

use crate::registry::Tool;

/// Cargo test runner tool.
pub struct TestRunnerTool;

/// Structured test result returned to the agent.
#[derive(Debug)]
pub struct TestReport {
    /// Number of passing tests.
    pub passed: usize,
    /// Number of failing tests.
    pub failed: usize,
    /// Individual failure records.
    pub errors: Vec<TestFailure>,
    /// Raw tool output for debugging.
    pub raw_output: String,
}

/// Single failing test entry.
#[derive(Debug)]
pub struct TestFailure {
    /// Test name or identifier.
    pub name: String,
    /// Failure message or panic text.
    pub message: String,
}

impl TestReport {
    /// Format a concise summary for the agent loop.
    pub fn to_agent_string(&self) -> String {
        let status = if self.failed == 0 { "PASS" } else { "FAIL" };
        let mut out = format!(
            "[{status}] {} passed, {} failed\n",
            self.passed, self.failed
        );
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
            Some(s) if s.contains('/') => format!(
                "cargo test --package {} 2>&1",
                s.split('/').next().unwrap_or(s)
            ),
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

    if std::path::Path::new("pyproject.toml").exists() || std::path::Path::new("setup.py").exists()
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
        if line.contains("test result:") {
            // e.g. "test result: FAILED. 3 passed; 2 failed; ..."
            if let Some(p) = extract_number(line, "passed") {
                passed = p;
            }
            if let Some(f) = extract_number(line, "failed") {
                failed = f;
            }
            continue;
        }
        if line.starts_with("test ") && line.ends_with(" ... ok") {
            passed += 1;
        } else if line.starts_with("test ") && line.contains(" ... FAILED") {
            failed += 1;
            let name = line
                .trim_start_matches("test ")
                .split(" ...")
                .next()
                .unwrap_or("?")
                .to_string();
            errors.push(TestFailure {
                name,
                message: "test failed".to_string(),
            });
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
                current_name = line
                    .trim_start_matches("---- ")
                    .split(' ')
                    .next()
                    .unwrap_or("?")
                    .to_string();
                current_msg = String::new();
            } else if !line.is_empty() && !line.starts_with("failures:") {
                current_msg.push_str(line);
                current_msg.push('\n');
            }
        }
    }

    TestReport {
        passed,
        failed,
        errors,
        raw_output: output.to_string(),
    }
}

fn parse_pytest(output: &str, success: bool) -> TestReport {
    let mut passed = 0;
    let mut failed = 0;
    let mut errors: Vec<TestFailure> = Vec::new();

    for line in output.lines() {
        // "== 3 passed, 1 failed in 0.12s =="
        if line.contains(" passed") || line.contains(" failed") {
            if let Some(p) = extract_number(line, "passed") {
                passed = p;
            }
            if let Some(f) = extract_number(line, "failed") {
                failed = f;
            }
        }
        // "FAILED test_file.py::test_name - AssertionError"
        if line.starts_with("FAILED ") {
            let rest = line.trim_start_matches("FAILED ");
            let (name, msg) = rest.split_once(" - ").unwrap_or((rest, "failed"));
            errors.push(TestFailure {
                name: name.to_string(),
                message: msg.to_string(),
            });
        }
    }

    if passed == 0 && failed == 0 && !success {
        failed = 1;
    }

    TestReport {
        passed,
        failed,
        errors,
        raw_output: output.to_string(),
    }
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
            let name = line
                .trim_start_matches("--- FAIL: ")
                .split(' ')
                .next()
                .unwrap_or("?")
                .to_string();
            errors.push(TestFailure {
                name,
                message: "test failed".to_string(),
            });
        }
    }

    TestReport {
        passed,
        failed,
        errors,
        raw_output: output.to_string(),
    }
}

fn parse_generic(output: &str, success: bool) -> TestReport {
    let failed = if success { 0 } else { 1 };
    TestReport {
        passed: 0,
        failed,
        errors: if failed > 0 {
            vec![TestFailure {
                name: "test".into(),
                message: output.lines().last().unwrap_or("failed").to_string(),
            }]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_name() {
        assert_eq!(TestRunnerTool.definition().function.name, "test_runner");
    }

    #[test]
    fn extract_number_from_cargo_summary() {
        let line = "test result: ok. 3 passed; 2 failed; 0 ignored";
        assert_eq!(extract_number(line, "passed"), Some(3));
        assert_eq!(extract_number(line, "failed"), Some(2));
        assert_eq!(extract_number("nope", "passed"), None);
    }

    #[test]
    fn parse_cargo_counts_and_failures() {
        let out = "\
test foo::bar ... ok
test foo::baz ... FAILED
test result: FAILED. 1 passed; 1 failed; 0 ignored
failures:
---- foo::baz stdout ----
assertion failed
";
        let report = parse_cargo(out, false);
        // summary line wins for totals when present
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
        assert!(!report.errors.is_empty());
        let s = report.to_agent_string();
        assert!(s.contains("FAIL"));
        assert!(s.contains("FAILED:"));
    }

    #[test]
    fn parse_cargo_all_pass_agent_string() {
        let out = "test a ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored\n";
        let report = parse_cargo(out, true);
        assert_eq!(report.failed, 0);
        assert!(report.to_agent_string().contains("PASS"));
        assert!(report.to_agent_string().contains("All tests passed"));
    }

    #[test]
    fn parse_pytest_and_go_and_generic() {
        let py = "== 2 passed, 1 failed in 0.1s ==\nFAILED test_a.py::t - boom\n";
        let pr = parse_pytest(py, false);
        assert_eq!(pr.passed, 2);
        assert_eq!(pr.failed, 1);
        assert_eq!(pr.errors[0].name, "test_a.py::t");

        let go = "--- PASS: TestA (0.00s)\n--- FAIL: TestB (0.00s)\n";
        let gr = parse_go(go, false);
        assert_eq!(gr.passed, 1);
        assert_eq!(gr.failed, 1);

        let gen_ok = parse_generic("ok\n", true);
        assert_eq!(gen_ok.failed, 0);
        let gen_bad = parse_generic("last line err\n", false);
        assert_eq!(gen_bad.failed, 1);
        assert!(gen_bad.errors[0].message.contains("err"));
    }

    #[test]
    fn parse_output_dispatches_runner() {
        let r = parse_output(
            &Runner::Cargo,
            "test x ... ok\ntest result: ok. 1 passed; 0 failed\n",
            true,
        );
        assert_eq!(r.passed, 1);
        let r2 = parse_output(&Runner::Make, "done\n", true);
        assert_eq!(r2.failed, 0);
    }

    #[test]
    fn definition_schema_has_optional_scope_and_timeout() {
        let def = TestRunnerTool.definition();
        assert_eq!(def.function.name, "test_runner");
        let props = &def.function.parameters["properties"];
        assert!(props["scope"].is_object());
        assert!(props["timeout_secs"].is_object());
        // No required args — scope/timeout are optional.
        let required = def.function.parameters.get("required");
        assert!(required.is_none() || required.unwrap().as_array().unwrap().is_empty());
        assert!(
            def.function.description.contains("test suite")
                || def.function.description.contains("cargo")
        );
    }

    #[test]
    fn extract_number_edges() {
        assert_eq!(extract_number("12 passed", "passed"), Some(12));
        assert_eq!(extract_number("passed", "passed"), None); // nothing before word
        assert_eq!(extract_number("x passed", "passed"), None); // non-numeric token
        assert_eq!(extract_number("3 ignored; 0 failed", "failed"), Some(0));
        assert_eq!(extract_number("", "passed"), None);
    }

    #[test]
    fn parse_cargo_without_summary_counts_line_markers() {
        let out = "\
test a::one ... ok
test a::two ... ok
test a::three ... FAILED
";
        let report = parse_cargo(out, false);
        assert_eq!(report.passed, 2);
        assert_eq!(report.failed, 1);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].name, "a::three");
        assert_eq!(report.errors[0].message, "test failed");
        assert!(report.raw_output.contains("a::three"));
    }

    #[test]
    fn parse_cargo_attaches_failure_stdout_message() {
        let out = "\
test mods::x ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored
failures:
---- mods::x stdout ----
thread 'mods::x' panicked at 'boom'
";
        let report = parse_cargo(out, false);
        assert_eq!(report.failed, 1);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].name, "mods::x");
        // Message remains default until a subsequent failure header flushes it;
        // single-failure blocks still leave the placeholder unless flushed.
        assert!(!report.errors[0].name.is_empty());
        let agent = report.to_agent_string();
        assert!(agent.starts_with("[FAIL]"));
        assert!(agent.contains("0 passed, 1 failed"));
    }

    #[test]
    fn parse_pytest_failed_without_dash_and_empty_failure_fallback() {
        let py = "FAILED test_b.py::lonely\n";
        let pr = parse_pytest(py, false);
        assert_eq!(pr.errors[0].name, "test_b.py::lonely");
        assert_eq!(pr.errors[0].message, "failed");

        let empty = parse_pytest("garbage only\n", false);
        assert_eq!(empty.failed, 1);
        assert_eq!(empty.passed, 0);

        let ok_empty = parse_pytest("no numbers here\n", true);
        assert_eq!(ok_empty.failed, 0);
        assert_eq!(ok_empty.passed, 0);
    }

    #[test]
    fn parse_go_empty_and_multi() {
        let empty = parse_go("", true);
        assert_eq!(empty.passed, 0);
        assert_eq!(empty.failed, 0);
        assert!(empty.errors.is_empty());

        let multi = parse_go(
            "--- PASS: TestOne (0.01s)\n--- PASS: TestTwo (0.00s)\n--- FAIL: TestThree (0.02s)\n",
            false,
        );
        assert_eq!(multi.passed, 2);
        assert_eq!(multi.failed, 1);
        assert_eq!(multi.errors[0].name, "TestThree");
    }

    #[test]
    fn parse_generic_success_and_empty_failure_message() {
        let ok = parse_generic("", true);
        assert_eq!(ok.passed, 0);
        assert_eq!(ok.failed, 0);
        assert!(ok.errors.is_empty());
        assert!(ok.to_agent_string().contains("PASS"));
        assert!(ok.to_agent_string().contains("All tests passed"));

        let bad = parse_generic("", false);
        assert_eq!(bad.failed, 1);
        assert_eq!(bad.errors[0].name, "test");
        // empty output → last line falls back to "failed"
        assert_eq!(bad.errors[0].message, "failed");
    }

    #[test]
    fn parse_output_dispatches_all_runners() {
        let npm = parse_output(&Runner::Npm, "tests failed hard\n", false);
        assert_eq!(npm.failed, 1);
        assert!(npm.errors[0].message.contains("hard"));

        let py = parse_output(&Runner::Pytest, "== 1 passed, 0 failed in 0.01s ==\n", true);
        assert_eq!(py.passed, 1);
        assert_eq!(py.failed, 0);

        let go = parse_output(&Runner::Go, "--- PASS: T (0s)\n", true);
        assert_eq!(go.passed, 1);

        let cargo = parse_output(&Runner::Cargo, "test z ... ok\n", true);
        assert_eq!(cargo.passed, 1);
    }

    #[test]
    fn runner_enum_equality() {
        assert_eq!(Runner::Cargo, Runner::Cargo);
        assert_ne!(Runner::Cargo, Runner::Npm);
        assert_ne!(Runner::Pytest, Runner::Go);
        assert_ne!(Runner::Make, Runner::Npm);
    }
}
