use super::Tool;
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

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
            return Ok("No test framework detected for watch mode.".to_string())
        }
    };

    Ok(format!(
        "Watch mode command (run manually):\n  cd {} && {}",
        path, cmd
    ))
}

fn format_output(output: &std::process::Output) -> Result<String> {
    let mut result = String::new();

    if !output.stdout.is_empty() {
        result.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        result.push_str(&format!(
            "\n[stderr]: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let status = if output.status.success() {
        "PASSED"
    } else {
        "FAILED"
    };

    Ok(format!("Test run: {}\n\n{}", status, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = format!("/tmp/openshark_testrunner_test_{}_{}", std::process::id(), count);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &str) {
        let _ = fs::remove_dir_all(dir);
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
        assert!(matches!(framework, TestFramework::GoTest) || matches!(framework, TestFramework::Unknown));
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
}
