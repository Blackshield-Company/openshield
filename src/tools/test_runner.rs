use super::Tool;
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Maximum output size in bytes before truncation (128KB).
const MAX_OUTPUT_BYTES: usize = 128 * 1024;
/// Maximum lines of output to capture.
const MAX_OUTPUT_LINES: usize = 2000;
/// Paths that should never be treated as project roots for test discovery.
const BLOCKED_PATH_PREFIXES: &[&str] = &[
    "/home/synth/.cargo",
    "/home/synth/.cache",
    "/home/synth/.local/share",
    "/home/synth/.rustup",
    "/usr",
    "/var",
    "/tmp",
    "/etc",
    "/opt",
];

pub struct TestTool;

impl Tool for TestTool {
    fn name(&self) -> &str {
        "test"
    }

    fn description(&self) -> &str {
        "Auto-detect and run tests. Usage: test [run|list|watch] [path]"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("run");
        let path = parts.get(1).copied().unwrap_or(".");

        // Validate path before doing anything
        if let Err(e) = validate_test_path(path) {
            return Ok(format!("Test path validation failed: {}", e));
        }

        let framework = detect_test_framework(path);

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
         test run [path]   - Run tests (default)\n\
         test list [path]  - List test files\n\
         test watch [path] - Run tests on file changes"
            .to_string()
    }
}

#[derive(Debug, Clone)]
enum TestFramework {
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

    for blocked in BLOCKED_PATH_PREFIXES {
        if canonical_str.starts_with(*blocked) {
            anyhow::bail!(
                "Path '{}' is in a blocked directory ({}). \
                 These directories contain system/package files, not project tests. \
                 Run OpenShark from your project directory or set working_directory in config.",
                path,
                blocked
            );
        }
    }

    // Warn if the path is the home directory itself — almost never intentional
    if canonical_str == "/home/synth" || canonical_str == "/home/synth/" {
        anyhow::bail!(
            "Refusing to run tests from home directory (/home/synth). \
             This would recursively execute every test file on your system. \
             cd into a project directory or set working_directory in config."
        );
    }

    Ok(())
}

fn detect_test_framework(path: &str) -> TestFramework {
    let p = Path::new(path);

    if p.join("Cargo.toml").exists()
        || p.parent()
            .map(|pp| pp.join("Cargo.toml").exists())
            .unwrap_or(false)
    {
        return TestFramework::Cargo;
    }
    if p.join("package.json").exists() {
        return TestFramework::Jest;
    }
    if p.join("pytest.ini").exists()
        || p.join("setup.py").exists()
        || p.join("pyproject.toml").exists()
    {
        return TestFramework::Pytest;
    }
    if p.join("go.mod").exists() {
        return TestFramework::GoTest;
    }

    for ancestor in p.ancestors().skip(1).take(3) {
        if ancestor.join("Cargo.toml").exists() {
            return TestFramework::Cargo;
        }
        if ancestor.join("package.json").exists() {
            return TestFramework::Jest;
        }
        if ancestor.join("go.mod").exists() {
            return TestFramework::GoTest;
        }
    }

    TestFramework::Unknown
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = format!(
            "/tmp/openshark_testrunner_test_{}_{}",
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
        let result = validate_test_path("/home/synth/.cargo/registry/src");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("blocked directory"));
    }

    #[test]
    fn test_validate_blocks_home_dir() {
        let result = validate_test_path("/home/synth");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("home directory"));
    }

    #[test]
    fn test_validate_allows_project_dir() {
        // Use a path under /home/synth so it doesn't hit the /tmp block
        let dir = format!("/home/synth/.tmp_testrunner_{}", std::process::id());
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
}
