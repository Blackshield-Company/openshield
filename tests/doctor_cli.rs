//! `openshield doctor --fix` must work when config.toml is broken: it is the
//! tool that repairs it, so it cannot depend on loading it first.
//!
//! Unix only: the config dir is redirected through HOME / XDG_CONFIG_HOME,
//! which Windows' known-folder lookup ignores.
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

fn run(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_openshield"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env_remove("KIMI_API_KEY")
        .output()
        .expect("run openshield")
}

/// Where `dirs::config_dir()` lands for this HOME.
fn config_dir(home: &Path) -> std::path::PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/openshield")
    } else {
        home.join(".config").join("openshield")
    }
}

/// A fresh, empty HOME under the cargo target tmp dir.
fn temp_home(name: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("doctor-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn broken_config_case(name: &str, contents: &str) {
    let home = temp_home(name);
    let dir = config_dir(&home);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), contents).unwrap();

    // Diagnose without fixing: exits 0 and names the problem.
    let out = run(&home, &["doctor"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "doctor failed: {stdout} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("CRITICAL"),
        "expected a critical Config check: {stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("config.toml")).unwrap(),
        contents
    );

    // Fix: backs the broken file up and writes a loadable default.
    let out = run(&home, &["doctor", "--fix"]);
    assert!(
        out.status.success(),
        "doctor --fix failed: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !contents.trim().is_empty() {
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml.bak")).unwrap(),
            contents,
            "broken config must be preserved in config.toml.bak"
        );
    }
    let fixed = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    toml::from_str::<openshield::config::Config>(&fixed).expect("fixed config loads");

    // And other commands work again.
    let out = run(&home, &["models"]);
    assert!(out.status.success(), "models still failing after fix");
}

#[test]
fn doctor_fix_repairs_invalid_toml() {
    broken_config_case(
        "invalid-toml",
        "default_model = \"x\"\n[providers.mine\nbroken\n",
    );
}

#[test]
fn doctor_fix_repairs_empty_config() {
    broken_config_case("empty", "");
}

#[test]
fn doctor_flags_valid_toml_that_is_not_a_config() {
    // Valid TOML, wrong shape: every command used to fail while doctor said healthy.
    broken_config_case("wrong-shape", "default_model = 5\n");
}

#[test]
fn broken_config_error_points_at_doctor() {
    let home = temp_home("error-hint");
    let dir = config_dir(&home);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), "garbage [[[").unwrap();
    let out = run(&home, &["models"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("openshield doctor --fix"), "{stderr}");
}
