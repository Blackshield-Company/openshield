//! `openshield headless --json` must write only NDJSON to stdout, so a CI
//! script can parse every line. Unix only: HOME / XDG redirection does not
//! move the config dir on Windows.
#![cfg(unix)]

mod common;

use common::MockProvider;
use openshield::config::{Config, ModelConfig, ProviderConfig, ProviderKind};

fn config_dir(home: &std::path::Path) -> std::path::PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/openshield")
    } else {
        home.join(".config").join("openshield")
    }
}

async fn run_headless(extra: &[&str]) -> (String, String) {
    let mock = MockProvider::start(vec![
        "Done. TASK_COMPLETE".to_string(),
        "TASK_COMPLETE".to_string(),
        "TASK_COMPLETE".to_string(),
    ])
    .await;
    let home = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "headless-json-{}-{}",
        std::process::id(),
        extra.join("")
    ));
    let _ = std::fs::remove_dir_all(&home);
    let dir = config_dir(&home);
    std::fs::create_dir_all(&dir).unwrap();
    let work = home.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut providers = std::collections::HashMap::new();
    providers.insert(
        "mock".to_string(),
        ProviderConfig {
            base_url: mock.base_url.clone(),
            api_key: "test-key".to_string(),
            models: vec![ModelConfig {
                name: "mock-model".to_string(),
                context_length: 8192,
                cost_per_1k_input: 0.0,
                cost_per_1k_output: 0.0,
                capabilities: vec!["code".to_string()],
            }],
            kind: ProviderKind::OpenAiCompatible,
            headers: std::collections::HashMap::new(),
            env_file: None,
        },
    );
    let config = Config {
        providers,
        memory_db_path: home.join("memory.db"),
        default_model: "mock-model".to_string(),
        ..Config::default()
    };
    let config = toml::to_string_pretty(&config).unwrap();
    std::fs::write(dir.join("config.toml"), config).unwrap();

    let mut args = vec!["headless", "--json", "-n", "2", "-t", "60"];
    args.extend_from_slice(extra);
    args.push("say hello");
    let args: Vec<String> = args.into_iter().map(String::from).collect();
    let out = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_openshield"))
            .args(&args)
            .current_dir(&work)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env_remove("KIMI_API_KEY")
            .output()
            .expect("run openshield")
    })
    .await
    .unwrap();
    assert!(
        out.status.success(),
        "headless failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn assert_ndjson(stdout: &str) {
    let mut events = 0;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("non-JSON line on stdout ({e}): {line:?}\n--- stdout ---\n{stdout}")
        });
        assert!(v.get("type").is_some(), "event without type: {line}");
        events += 1;
    }
    assert!(stdout.contains("\"type\":\"start\""), "{stdout}");
    assert!(stdout.contains("\"type\":\"complete\""), "{stdout}");
    assert!(events >= 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn headless_json_stdout_is_pure_ndjson() {
    let (stdout, stderr) = run_headless(&[]).await;
    assert_ndjson(&stdout);
    assert!(
        stderr.contains("Headless Mode"),
        "banner should move to stderr"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn headless_json_yolo_stdout_is_pure_ndjson() {
    // --yolo also runs auto-test, auto-commit and the self-improvement
    // report, which used to print its tables to stdout.
    let (stdout, _) = run_headless(&["--yolo"]).await;
    assert_ndjson(&stdout);
    assert!(!stdout.contains("Self-Improvement"), "{stdout}");
}
