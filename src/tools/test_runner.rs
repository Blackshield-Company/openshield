use super::Tool;
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Maximum output size in bytes before truncation (128KB).
const MAX_OUTPUT_BYTES: usize = 128 * 1024;
/// Maximum lines of output to capture.
const MAX_OUTPUT_LINES: usize = 2000;
/// Paths that should never be treated as project roots for test discovery.
/// System dirs are static; the home-relative entries are derived from the
/// ACTUAL home directory (not hardcoded) so the guard works for any user.
fn blocked_path_prefixes() -> Vec<String> {
    let mut prefixes: Vec<String> = ["/usr", "/var", "/tmp", "/etc", "/opt"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(home) = dirs::home_dir() {
        let home = home.to_string_lossy().to_string();
        for sub in [".cargo", ".cache", ".local/share", ".rustup"] {
            prefixes.push(format!("{}/{}", home, sub));
        }
    }
    prefixes
}

pub struct TestTool;

impl Tool for TestTool {
    fn name(&self) -> &str {
        "test"
    }

    fn description(&self) -> &str {
        "Auto-detect and run tests. Usage: test [run|list|watch] [path] [framework]. \
         Framework is optional (cargo|jest|pytest|go) and overrides auto-detection."
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("run");
        let path = parts.get(1).copied().unwrap_or(".");

        // Optional explicit framework: `test run <path> pytest`. A hint the
        // model gives us MUST be honored over auto-detection — detection
        // guesses from directory markers and can pick the parent project's
        // framework when the actual target is a scratch dir inside it.
        let explicit = match parts.get(2) {
            None => None,
            Some(f) => match parse_framework(f) {
                Some(fw) => Some(fw),
                None => {
                    return Ok(format!(
                        "Unknown test framework: '{}'. Supported: cargo, jest, pytest, go\n{}",
                        f,
                        self.usage()
                    ));
                }
            },
        };

        // Validate path before doing anything
        if let Err(e) = validate_test_path(path) {
            return Ok(format!("Test path validation failed: {}", e));
        }

        let framework = explicit.unwrap_or_else(|| detect_test_framework(path));

        match cmd {
            "run" => run_tests(path, &framework),
            "list" => list_tests(path, &framework),
            "watch" => watch_tests(path, &framework),
            _ => Ok(format!("Unknown test command: {}\n{}", cmd, self.usage())),
        }
    }
}

impl TestTool {
    fn usage(&self) -> String {
        "Test tool usage:\n\
         test run [path] [framework]   - Run tests (default)\n\
         test list [path] [framework]  - List test files\n\
         test watch [path] [framework] - Run tests on file changes\n\
         framework: cargo|jest|pytest|go (optional, overrides auto-detection)"
            .to_string()
    }
}

/// Parse an explicit framework name from the command line.
fn parse_framework(s: &str) -> Option<TestFramework> {
    match s.to_ascii_lowercase().as_str() {
        "cargo" | "rust" => Some(TestFramework::Cargo),
        "jest" | "npm" | "node" => Some(TestFramework::Jest),
        "pytest" | "python" => Some(TestFramework::Pytest),
        "go" | "gotest" | "go-test" => Some(TestFramework::GoTest),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TestFramework {
    Cargo,
    Jest,
    Pytest,
    GoTest,
    Unknown,
}

fn validate_test_path(path: &str) -> Result<()> {
    let canonical = std::fs::canonicalize(path)
        .with_context(|| format!("Cannot resolve test path: {}", path))?;

    let canonical_str = canonical.to_string_lossy();

    for blocked in blocked_path_prefixes() {
        if canonical_str.starts_with(blocked.as_str()) {
            anyhow::bail!(
                "Path '{}' is in a blocked directory ({}). \
                 These directories contain system/package files, not project tests. \
                 Run OpenShield from your project directory or set working_directory in config.",
                path,
                blocked
            );
        }
    }

    // Warn if the path is the home directory itself — almost never intentional
    if let Some(home) = dirs::home_dir()
        && canonical == home
    {
        anyhow::bail!(
            "Refusing to run tests from home directory ({}). \
                 This would recursively execute every test file on your system. \
                 cd into a project directory or set working_directory in config.",
            home.display()
        );
    }

    Ok(())
}

fn detect_test_framework(path: &str) -> TestFramework {
    let p = Path::new(path);

    // 1. Markers at the exact path always win — the caller pointed here on
    //    purpose. Previously a parent Cargo.toml outranked an exact-path
    //    package.json/pytest.ini, which mis-detected scratch dirs inside
    //    foreign repos.
    if let Some(f) = detect_markers_at(p) {
        return f;
    }

    // 2. Nearest ancestor wins (up to 3 levels), all marker kinds checked at
    //    each level — pytest markers included (they used to be exact-path
    //    only, so `repo-with-pytest.ini/subdir` detected as Cargo if a Rust
    //    project lived closer).
    for ancestor in p.ancestors().skip(1).take(3) {
        if let Some(f) = detect_markers_at(ancestor) {
            return f;
        }
    }

    TestFramework::Unknown
}

/// Detect a test framework from marker files in a single directory.
fn detect_markers_at(dir: &Path) -> Option<TestFramework> {
    if dir.join("Cargo.toml").exists() {
        return Some(TestFramework::Cargo);
    }
    if dir.join("package.json").exists() {
        return Some(TestFramework::Jest);
    }
    if dir.join("pytest.ini").exists()
        || dir.join("setup.py").exists()
        || dir.join("pyproject.toml").exists()
    {
        return Some(TestFramework::Pytest);
    }
    if dir.join("go.mod").exists() {
        return Some(TestFramework::GoTest);
    }
    None
}

fn run_tests(path: &str, framework: &TestFramework) -> Result<String> {
    let output = match framework {
        TestFramework::Cargo => Command::new("cargo")
            .args(["test", "--", "--nocapture"])
            .current_dir(path)
            .output(),
        TestFramework::Jest => Command::new("npx")
            .args(["jest", "--verbose", "--no-coverage"])
            .current_dir(path)
            .output(),
        TestFramework::Pytest => Command::new("pytest")
            .args(["-v"])
            .current_dir(path)
            .output(),
        TestFramework::GoTest => Command::new("go")
            .args(["test", "-v", "./..."])
            .current_dir(path)
            .output(),
        TestFramework::Unknown => {
            return Ok("No test framework detected. Looked for: Cargo.toml, package.json, pytest.ini, go.mod".to_string());
        }
    };

    let output = output.with_context(|| "Failed to run tests")?;
    format_output(&output)
}

fn list_tests(path: &str, framework: &TestFramework) -> Result<String> {
    let output = match framework {
        TestFramework::Cargo => Command::new("cargo")
            .args(["test", "--", "--list"])
            .current_dir(path)
            .output(),
        TestFramework::Jest => Command::new("npx")
            .args(["jest", "--listTests"])
            .current_dir(path)
            .output(),
        TestFramework::Pytest => Command::new("pytest")
            .args(["--collect-only"])
            .current_dir(path)
            .output(),
        TestFramework::GoTest => Command::new("go")
            .args(["test", "-list=.", "./..."])
            .current_dir(path)
            .output(),
        TestFramework::Unknown => {
            return Ok("No test framework detected.".to_string());
        }
    };

    let output = output.with_context(|| "Failed to list tests")?;
    format_output(&output)
}

fn watch_tests(path: &str, framework: &TestFramework) -> Result<String> {
    let cmd = match framework {
        TestFramework::Cargo => "cargo watch -x test",
        TestFramework::Jest => "npx jest --watch",
        TestFramework::Pytest => "pytest-watch",
        TestFramework::GoTest => "gow test ./...",
        TestFramework::Unknown => {
            return Ok("No test framework detected for watch mode.".to_string());
        }
    };

    Ok(format!(
        "Watch mode command (run manually):\n  cd {} && {}",
        path, cmd
    ))
}

fn format_output(output: &std::process::Output) -> Result<String> {
    let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Truncate stdout if too large
    if stdout.len() > MAX_OUTPUT_BYTES {
        let truncated = truncate_to_lines(&stdout, MAX_OUTPUT_LINES);
        stdout = format!(
            "{}\n\n[TRUNCATED: output exceeded {} lines / {}KB limit]",
            truncated,
            MAX_OUTPUT_LINES,
            MAX_OUTPUT_BYTES / 1024
        );
    }

    // Truncate stderr if too large
    if stderr.len() > MAX_OUTPUT_BYTES {
        let truncated = truncate_to_lines(&stderr, MAX_OUTPUT_LINES);
        stderr = format!(
            "{}\n\n[TRUNCATED: stderr exceeded {} lines / {}KB limit]",
            truncated,
            MAX_OUTPUT_LINES,
            MAX_OUTPUT_BYTES / 1024
        );
    }

    let mut result = String::new();

    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        result.push_str(&format!("\n[stderr]: {}", stderr));
    }

    let status = if output.status.success() {
        "PASSED"
    } else {
        "FAILED"
    };

    Ok(format!("Test run: {}\n\n{}", status, result))
}

/// Truncate a string to at most `max_lines` lines, preserving the start.
fn truncate_to_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }
    let kept: Vec<&str> = lines.into_iter().take(max_lines).collect();
    kept.join("\n")
}

/// Parsed test result for programmatic consumption.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TestResultSet {
    pub framework: String,
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
    pub total: usize,
    pub duration_secs: Option<f64>,
    pub failures: Vec<TestFailure>,
    pub raw_output: String,
    pub success: bool,
}

/// A single test failure.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TestFailure {
    pub test_name: String,
    pub module: Option<String>,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Parse test output into a structured result set.
/// Supports cargo test, pytest, jest, and go test output formats.
pub(crate) fn parse_test_output(raw: &str, framework: &TestFramework) -> TestResultSet {
    let framework_name = match framework {
        TestFramework::Cargo => "cargo",
        TestFramework::Jest => "jest",
        TestFramework::Pytest => "pytest",
        TestFramework::GoTest => "go",
        TestFramework::Unknown => "unknown",
    };

    let mut result = TestResultSet {
        framework: framework_name.to_string(),
        passed: 0,
        failed: 0,
        ignored: 0,
        total: 0,
        duration_secs: None,
        failures: Vec::new(),
        raw_output: raw.to_string(),
        success: true,
    };

    match framework {
        TestFramework::Cargo => parse_cargo_output(raw, &mut result),
        TestFramework::Pytest => parse_pytest_output(raw, &mut result),
        TestFramework::Jest => parse_jest_output(raw, &mut result),
        TestFramework::GoTest => parse_go_output(raw, &mut result),
        TestFramework::Unknown => {
            // Try to infer from content
            if raw.contains("test result:") && raw.contains("cargo") {
                parse_cargo_output(raw, &mut result);
            } else if raw.contains("failed") || raw.contains("FAILED") {
                result.failed = raw.matches("FAILED").count();
                result.success = result.failed == 0;
            }
        }
    }

    result.total = result.passed + result.failed + result.ignored;
    result
}

fn parse_cargo_output(raw: &str, result: &mut TestResultSet) {
    // Parse lines like: "test result: ok. 420 passed; 0 failed; 3 ignored;"
    for line in raw.lines() {
        if line.contains("test result:") {
            if let Some(passed) = extract_count(line, "passed") {
                result.passed = passed;
            }
            if let Some(failed) = extract_count(line, "failed") {
                result.failed = failed;
            }
            if let Some(ignored) = extract_count(line, "ignored") {
                result.ignored = ignored;
            }
            if line.contains("ok.") {
                result.success = result.failed == 0;
            } else {
                result.success = false;
            }
        }

        // Parse failures: "test test_name ... FAILED"
        if line.contains("... FAILED") || line.contains("...FAILED") {
            let name = line.split("...").next().unwrap_or("").trim();
            if !name.is_empty() && name != "test" {
                result.failures.push(TestFailure {
                    test_name: name.trim_start_matches("test ").to_string(),
                    module: None,
                    message: String::new(),
                    file: None,
                    line: None,
                });
            }
        }

        // Parse panic locations: "thread 'test_name' panicked at 'message', src/file.rs:42:5"
        if line.contains("panicked at") {
            let name_start = line.find("thread '").map(|i| i + 8);
            let name_end = line.find("' panicked at");
            if let (Some(s), Some(e)) = (name_start, name_end) {
                let test_name = &line[s..e];
                // Find the file:line
                let rest = &line[e + "' panicked at ".len()..];
                let file_line = rest.split(", ").nth(1).unwrap_or("");
                // file_line is like "src/file.rs:42:5" — split from right to get file:line
                let parts: Vec<&str> = file_line.rsplitn(3, ':').collect();
                // parts: ["5", "42", "src/file.rs"] (reversed from right)
                let line_num = parts.get(1).and_then(|l| l.trim().parse::<u32>().ok());
                let file = parts
                    .get(2)
                    .map(|f| f.to_string())
                    .or_else(|| parts.get(1).map(|f| f.to_string()));
                let msg = rest.split("'").nth(1).unwrap_or("").to_string();

                // Add or update failure
                if let Some(failure) = result
                    .failures
                    .iter_mut()
                    .find(|f| f.test_name == test_name)
                {
                    failure.message = msg;
                    failure.file = file;
                    failure.line = line_num;
                } else {
                    result.failures.push(TestFailure {
                        test_name: test_name.to_string(),
                        module: None,
                        message: msg,
                        file,
                        line: line_num,
                    });
                }
            }
        }
    }
}

fn parse_pytest_output(raw: &str, result: &mut TestResultSet) {
    for line in raw.lines() {
        // Parse summary line: "X passed, Y failed, Z skipped in N.NNs"
        if line.contains("passed") || line.contains("failed") {
            if let Some(p) = extract_count(line, "passed") {
                result.passed = p;
            }
            if let Some(f) = extract_count(line, "failed") {
                result.failed = f;
            }
            if let Some(s) = extract_count(line, "skipped") {
                result.ignored = s;
            }
            // Duration
            if let Some(idx) = line.rfind("in ") {
                let dur_str = &line[idx + 3..];
                let dur_num: String = dur_str
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                result.duration_secs = dur_num.parse().ok();
            }
            result.success = result.failed == 0;
        }

        // Parse failure: "FAILED test_file.py::test_name - AssertionError: ..."
        if let Some(rest) = line.strip_prefix("FAILED ") {
            let parts: Vec<&str> = rest.splitn(2, " - ").collect();
            let full_name = parts.first().unwrap_or(&"");
            let message = parts.get(1).unwrap_or(&"").to_string();
            let name_parts: Vec<&str> = full_name.split("::").collect();
            let (module, test_name) = if name_parts.len() > 1 {
                (Some(name_parts[0].to_string()), name_parts[1].to_string())
            } else {
                (None, full_name.to_string())
            };
            result.failures.push(TestFailure {
                test_name,
                module,
                message,
                file: None,
                line: None,
            });
        }
    }
}

fn parse_jest_output(raw: &str, result: &mut TestResultSet) {
    for line in raw.lines() {
        // Jest summary: "Tests: X failed, Y passed, Z total"
        if line.starts_with("Tests:") {
            if let Some(f) = extract_count(line, "failed") {
                result.failed = f;
            }
            if let Some(p) = extract_count(line, "passed") {
                result.passed = p;
            }
            if let Some(t) = extract_count(line, "total") {
                result.total = t;
            }
            result.success = result.failed == 0;
        }
        // Jest time: "Time: 3.456s"
        if let Some(rest) = line.strip_prefix("Time:") {
            let dur: String = rest
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            result.duration_secs = dur.parse().ok();
        }
        // Parse FAIL lines
        let name = if let Some(rest) = line.strip_prefix("FAIL ") {
            Some(rest)
        } else if line.starts_with("  ✕ ") || line.starts_with("  ✗ ") || line.starts_with("  × ")
        {
            Some(line.trim_start_matches([' ', '✕', '✗', '×']).trim_start())
        } else {
            None
        };
        if let Some(name) = name {
            result.failures.push(TestFailure {
                test_name: name.trim().to_string(),
                module: None,
                message: String::new(),
                file: None,
                line: None,
            });
        }
    }
}

fn parse_go_output(raw: &str, result: &mut TestResultSet) {
    for line in raw.lines() {
        // Go: "--- FAIL: TestName (0.00s)"
        if line.contains("--- FAIL:") {
            let rest = &line[line.find("--- FAIL:").unwrap() + 9..];
            let name = rest.split('(').next().unwrap_or("").trim();
            result.failures.push(TestFailure {
                test_name: name.to_string(),
                module: None,
                message: String::new(),
                file: None,
                line: None,
            });
        }
        // Go summary: "FAIL\t packageName\t 0.123s"
        if line.starts_with("FAIL") && line.contains('\t') {
            result.success = false;
        }
        // Go: "ok  \t packageName\t 0.123s"
        if line.starts_with("ok") && line.contains('\t') {
            result.passed += 1;
        }
        // Go: "FAIL\t packageName\t 0.123s"
        if line.starts_with("FAIL\t") {
            result.failed += 1;
        }
    }
}

/// Extract a count from text like "42 passed" or "3 failed".
fn extract_count(text: &str, label: &str) -> Option<usize> {
    let pattern = format!(" {}", label);
    if let Some(idx) = text.find(&pattern) {
        let before = &text[..idx];
        let num_str: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return num_str.parse().ok();
    }
    None
}

/// Run tests and return structured results.
/// This is the agent-friendly API that returns typed data instead of raw strings.
pub fn run_tests_structured(path: &str) -> Result<TestResultSet> {
    let framework = detect_test_framework(path);
    let raw = run_tests(path, &framework)?;
    Ok(parse_test_output(&raw, &framework))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = format!(
            "/tmp/openshield_testrunner_test_{}_{}",
            std::process::id(),
            count
        );
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &str) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_validate_blocks_cargo_registry() {
        let home = dirs::home_dir().expect("home dir");
        let registry = home.join(".cargo/registry/src");
        let registry = registry.to_string_lossy().to_string();
        let result = validate_test_path(&registry);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("blocked directory"));
    }

    #[test]
    fn test_validate_blocks_home_dir() {
        let home = dirs::home_dir().expect("home dir");
        let result = validate_test_path(&home.to_string_lossy());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("home directory"));
    }

    #[test]
    fn test_validate_blocks_are_derived_from_home_dir() {
        // The guard must not depend on a hardcoded user: any path under the
        // real home dir's .cargo/.cache/.local/share/.rustup is blocked, and
        // a sibling directory directly under home is allowed.
        let home = dirs::home_dir().expect("home dir");
        let blocked = home.join(".rustup/toolchains");
        assert!(validate_test_path(&blocked.to_string_lossy()).is_err());

        let allowed = home.join(format!(".tmp_testrunner_sibling_{}", std::process::id()));
        let _ = fs::remove_dir_all(&allowed);
        fs::create_dir_all(&allowed).unwrap();
        // A bare directory under home is not a project (no test markers), but
        // validation itself must pass — framework detection happens later.
        assert!(
            validate_test_path(&allowed.to_string_lossy()).is_ok(),
            "home-dir guard must only block the home dir itself, not its children"
        );
        let _ = fs::remove_dir_all(&allowed);
    }

    #[test]
    fn test_validate_allows_project_dir() {
        // Use a path under the real home dir so it doesn't hit the /tmp block
        let dir = dirs::home_dir()
            .expect("home dir")
            .join(format!(".tmp_testrunner_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(format!("{}/Cargo.toml", dir), "[package]\nname = \"test\"").unwrap();
        let result = validate_test_path(&dir);
        assert!(result.is_ok(), "Expected ok, got: {:?}", result);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_detect_cargo_framework() {
        let dir = temp_dir();
        fs::write(format!("{}/Cargo.toml", dir), "[package]\nname = \"test\"").unwrap();

        let framework = detect_test_framework(&dir);
        assert!(matches!(framework, TestFramework::Cargo));
        cleanup(&dir);
    }

    #[test]
    fn test_detect_jest_framework() {
        let dir = temp_dir();
        fs::write(format!("{}/package.json", dir), "{}").unwrap();

        let framework = detect_test_framework(&dir);
        assert!(matches!(framework, TestFramework::Jest));
        cleanup(&dir);
    }

    #[test]
    fn test_detect_pytest_framework() {
        let dir = temp_dir();
        fs::write(format!("{}/pytest.ini", dir), "[pytest]").unwrap();

        let framework = detect_test_framework(&dir);
        assert!(matches!(framework, TestFramework::Pytest));
        cleanup(&dir);
    }

    #[test]
    fn test_detect_go_framework() {
        let dir = temp_dir();
        fs::write(format!("{}/go.mod", dir), "module test").unwrap();

        let framework = detect_test_framework(&dir);
        assert!(
            matches!(framework, TestFramework::GoTest)
                || matches!(framework, TestFramework::Unknown)
        );
        cleanup(&dir);
    }

    #[test]
    fn test_detect_unknown_framework() {
        let dir = temp_dir();
        let framework = detect_test_framework(&dir);
        assert!(matches!(framework, TestFramework::Unknown));
        cleanup(&dir);
    }

    #[test]
    fn test_detect_cargo_in_parent() {
        let dir = temp_dir();
        let subdir = format!("{}/src", dir);
        fs::create_dir_all(&subdir).unwrap();
        fs::write(format!("{}/Cargo.toml", dir), "[package]\nname = \"test\"").unwrap();

        let framework = detect_test_framework(&subdir);
        assert!(matches!(framework, TestFramework::Cargo));
        cleanup(&dir);
    }

    #[test]
    fn test_watch_tests_cargo() {
        let result = watch_tests(".", &TestFramework::Cargo).unwrap();
        assert!(result.contains("cargo watch"));
    }

    #[test]
    fn test_watch_tests_unknown() {
        let result = watch_tests(".", &TestFramework::Unknown).unwrap();
        assert!(result.contains("No test framework"));
    }

    #[test]
    fn test_tool_usage() {
        let tool = TestTool;
        let result = tool.execute("unknown").unwrap();
        assert!(result.contains("Unknown test command"));
    }

    #[test]
    fn test_truncate_to_lines() {
        let text = "line1\nline2\nline3\nline4\nline5";
        assert_eq!(truncate_to_lines(text, 3), "line1\nline2\nline3");
        assert_eq!(truncate_to_lines(text, 10), text);
    }

    #[test]
    fn test_parse_cargo_output_passing() {
        let raw = "running 420 tests\ntest foo ... ok\ntest bar ... ok\n\ntest result: ok. 420 passed; 0 failed; 3 ignored; finished in 2.00s\n";
        let result = parse_test_output(raw, &TestFramework::Cargo);
        assert!(result.success);
        assert_eq!(result.passed, 420);
        assert_eq!(result.failed, 0);
        assert_eq!(result.ignored, 3);
        assert!(result.failures.is_empty());
    }

    #[test]
    fn test_parse_cargo_output_with_failures() {
        let raw = "running 5 tests\ntest test_a ... ok\ntest test_b ... FAILED\ntest test_c ... ok\n\ntest result: FAILED. 3 passed; 2 failed; 0 ignored;\nthread 'test_b' panicked at 'assertion failed', src/lib.rs:42:5\n";
        let result = parse_test_output(raw, &TestFramework::Cargo);
        assert!(!result.success);
        assert_eq!(result.passed, 3);
        assert_eq!(result.failed, 2);
        assert!(!result.failures.is_empty());
        // Check the panic was captured
        let panic_failure = result.failures.iter().find(|f| f.test_name == "test_b");
        assert!(panic_failure.is_some());
        let pf = panic_failure.unwrap();
        assert_eq!(pf.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(pf.line, Some(42));
    }

    #[test]
    fn test_parse_pytest_output() {
        let raw = "test_foo.py ..F\n\nFAILED test_foo.py::test_bar - AssertionError: expected 1\n\n2 passed, 1 failed in 0.05s\n";
        let result = parse_test_output(raw, &TestFramework::Pytest);
        assert!(!result.success);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
        assert_eq!(result.duration_secs, Some(0.05));
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].test_name, "test_bar");
        assert_eq!(result.failures[0].module.as_deref(), Some("test_foo.py"));
    }

    #[test]
    fn test_parse_jest_output() {
        let raw = "Tests: 1 failed, 5 passed, 6 total\nTime: 3.456s\nFAIL src/foo.test.js\n  ✕ should work\n";
        let result = parse_test_output(raw, &TestFramework::Jest);
        assert!(!result.success);
        assert_eq!(result.passed, 5);
        assert_eq!(result.failed, 1);
        assert_eq!(result.total, 6);
        assert_eq!(result.duration_secs, Some(3.456));
    }

    #[test]
    fn test_parse_go_output() {
        let raw = "--- FAIL: TestFoo (0.00s)\nok\tgithub.com/example/pkg\t0.123s\nFAIL\tgithub.com/example/bad\t0.456s\n";
        let result = parse_test_output(raw, &TestFramework::GoTest);
        assert!(!result.success);
        assert_eq!(result.passed, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].test_name, "TestFoo");
    }

    #[test]
    fn test_extract_count() {
        assert_eq!(extract_count("420 passed; 0 failed", "passed"), Some(420));
        assert_eq!(extract_count("420 passed; 0 failed", "failed"), Some(0));
        assert_eq!(extract_count("3 failed", "failed"), Some(3));
        assert_eq!(extract_count("no match here", "passed"), None);
    }

    #[test]
    fn test_parse_framework_aliases() {
        assert!(matches!(
            parse_framework("cargo"),
            Some(TestFramework::Cargo)
        ));
        assert!(matches!(
            parse_framework("rust"),
            Some(TestFramework::Cargo)
        ));
        assert!(matches!(parse_framework("JEST"), Some(TestFramework::Jest)));
        assert!(matches!(
            parse_framework("pytest"),
            Some(TestFramework::Pytest)
        ));
        assert!(matches!(
            parse_framework("python"),
            Some(TestFramework::Pytest)
        ));
        assert!(matches!(parse_framework("go"), Some(TestFramework::GoTest)));
        assert!(parse_framework("mocha").is_none());
    }

    #[test]
    fn test_exact_path_markers_beat_parent_cargo() {
        // Regression: a scratch dir with its own python markers inside a Rust
        // repo must detect as Pytest, not Cargo (parent used to win).
        let dir = temp_dir();
        let root = std::path::Path::new(&dir);
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let scratch = root.join("scratch");
        fs::create_dir_all(&scratch).unwrap();
        fs::write(scratch.join("pytest.ini"), "[pytest]\n").unwrap();

        assert!(matches!(
            detect_test_framework(scratch.to_str().unwrap()),
            TestFramework::Pytest
        ));
        cleanup(&dir);
    }

    #[test]
    fn test_ancestor_pytest_markers_detected() {
        // Regression: pytest markers were never checked on ancestors, so
        // <python-repo>/<subdir> detected as Unknown/Cargo instead of Pytest.
        let dir = temp_dir();
        let root = std::path::Path::new(&dir);
        fs::write(root.join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
        let sub = root.join("a/b");
        fs::create_dir_all(&sub).unwrap();

        assert!(matches!(
            detect_test_framework(sub.to_str().unwrap()),
            TestFramework::Pytest
        ));
        cleanup(&dir);
    }

    #[test]
    fn test_execute_rejects_unknown_framework() {
        let dir = temp_dir();
        let tool = TestTool;
        let out = tool.execute(&format!("run {} mocha", dir)).unwrap();
        assert!(out.contains("Unknown test framework"));
        assert!(out.contains("cargo, jest, pytest, go"));
        cleanup(&dir);
    }
}
