//! Watch Mode — Auto-run commands on file changes.
//!
//! Uses polling to watch the project directory and trigger
//! actions (test, lint, build) when files change.

#![allow(dead_code)]

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

/// Watch configuration.
#[derive(Debug, Clone)]
pub struct WatchConfig {
    pub path: String,
    pub debounce_ms: u64,
    pub command: WatchCommand,
}

#[derive(Debug, Clone)]
pub enum WatchCommand {
    Test,
    Lint,
    Build,
    Custom(String),
}

impl std::fmt::Display for WatchCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WatchCommand::Test => write!(f, "test"),
            WatchCommand::Lint => write!(f, "lint"),
            WatchCommand::Build => write!(f, "build"),
            WatchCommand::Custom(c) => write!(f, "{}", c),
        }
    }
}

/// Run watch mode until interrupted.
pub fn run_watch(config: WatchConfig) -> Result<()> {
    println!(
        "👁️  Watching {} for changes... (Ctrl+C to stop)",
        config.path
    );
    println!("   Trigger: {}", config.command);

    let mut last_run = Instant::now();
    let debounce = Duration::from_millis(config.debounce_ms);
    let mut file_times: HashMap<std::path::PathBuf, SystemTime> = HashMap::new();

    // Initial scan
    scan_dir(Path::new(&config.path), &mut file_times)?;

    loop {
        std::thread::sleep(Duration::from_millis(500));

        let mut changed = Vec::new();
        let mut new_times: HashMap<std::path::PathBuf, SystemTime> = HashMap::new();
        scan_dir(Path::new(&config.path), &mut new_times)?;

        for (path, time) in &new_times {
            match file_times.get(path) {
                Some(old_time) if old_time != time => {
                    changed.push(path.clone());
                }
                None => {
                    changed.push(path.clone());
                }
                _ => {}
            }
        }

        // Detect deleted files
        for path in file_times.keys() {
            if !new_times.contains_key(path) {
                changed.push(path.clone());
            }
        }

        if !changed.is_empty() && last_run.elapsed() >= debounce {
            last_run = Instant::now();
            println!();
            println!("🔄 Change detected: {:?}", changed);
            match run_command(&config.command) {
                Ok(output) => println!("{}", output),
                Err(e) => eprintln!("❌ Command failed: {}", e),
            }
            println!();
            println!("👁️  Watching...");
        }

        file_times = new_times;
    }
}

fn scan_dir(dir: &Path, out: &mut HashMap<std::path::PathBuf, SystemTime>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .map(|n| n == "target" || n == ".git")
                .unwrap_or(false)
            {
                continue;
            }
            scan_dir(&path, out)?;
        } else {
            if let Ok(meta) = entry.metadata()
                && let Ok(modified) = meta.modified()
            {
                out.insert(path, modified);
            }
        }
    }
    Ok(())
}

fn run_command(cmd: &WatchCommand) -> Result<String> {
    let output = match cmd {
        WatchCommand::Test => std::process::Command::new("cargo")
            .args(["test"])
            .output()?,
        WatchCommand::Lint => std::process::Command::new("cargo")
            .args(["clippy", "--", "-D", "warnings"])
            .output()?,
        WatchCommand::Build => std::process::Command::new("cargo")
            .args(["build"])
            .output()?,
        WatchCommand::Custom(c) => {
            let parts: Vec<&str> = c.split_whitespace().collect();
            if parts.is_empty() {
                anyhow::bail!("Empty custom command");
            }
            let mut command = std::process::Command::new(parts[0]);
            command.args(&parts[1..]);
            command.output()?
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        anyhow::bail!("{}\n{}", stdout, stderr);
    }

    Ok(if stdout.is_empty() { stderr } else { stdout })
}
