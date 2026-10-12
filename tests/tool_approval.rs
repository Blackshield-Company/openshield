//! Security gate: tool approval / deny / ask decisions.

use openshield::security::{
    PermissionLevel, RiskLevel, SecurityConfig, SecurityDecision, SecurityEngine,
};

fn engine_with(f: impl FnOnce(&mut SecurityConfig)) -> SecurityEngine {
    let mut cfg = SecurityConfig {
        working_directory: Some(std::env::current_dir().unwrap()),
        ..SecurityConfig::default()
    };
    f(&mut cfg);
    SecurityEngine::new(cfg).unwrap()
}

/// An absolute path inside the working directory (the sandbox allows it).
fn inside(rel: &str) -> String {
    std::env::current_dir()
        .unwrap()
        .join(rel)
        .to_string_lossy()
        .to_string()
}

fn engine() -> SecurityEngine {
    engine_with(|_| {})
}

fn is_allow(d: &SecurityDecision) -> bool {
    matches!(d, SecurityDecision::Allow)
}
fn is_deny(d: &SecurityDecision) -> bool {
    matches!(d, SecurityDecision::Deny { .. })
}
fn is_ask(d: &SecurityDecision) -> bool {
    matches!(d, SecurityDecision::RequireApproval { .. })
}

#[test]
fn safe_read_inside_workdir_is_allowed() {
    let d = engine().check_tool_call_with_mode("fs", "read Cargo.toml", false, false);
    assert!(is_allow(&d), "{d:?}");
}

#[test]
fn unknown_tools_default_to_ask_permission() {
    let cfg = SecurityConfig::default();
    assert_eq!(
        cfg.get_tool_permission("some_new_tool"),
        PermissionLevel::Ask
    );
    assert_eq!(cfg.get_tool_permission("fs"), PermissionLevel::Allow);
}

#[test]
fn denied_tool_is_blocked_even_in_yolo_mode() {
    let e = engine_with(|c| {
        c.tool_permissions
            .insert("terminal".into(), PermissionLevel::Deny);
    });
    for (auto, yolo) in [(false, false), (true, false), (false, true), (true, true)] {
        let d = e.check_tool_call_with_mode("terminal", "ls", auto, yolo);
        assert!(is_deny(&d), "auto={auto} yolo={yolo}: {d:?}");
    }
}

#[test]
fn sudo_is_never_auto_approved() {
    let e = engine();
    for (auto, yolo) in [(false, false), (true, false), (false, true)] {
        let d = e.check_tool_call_with_mode("terminal", "sudo rm -rf /var/log", auto, yolo);
        assert!(!is_allow(&d), "auto={auto} yolo={yolo}: {d:?}");
    }
}

#[test]
fn destructive_command_needs_approval_unless_yolo() {
    let e = engine();
    let cmd = format!("rm -rf {}", inside("target/scratch-build"));
    let d = e.check_tool_call_with_mode("terminal", &cmd, false, false);
    assert!(!is_allow(&d), "{d:?}");
    let d = e.check_tool_call_with_mode("terminal", &cmd, false, true);
    assert!(
        is_allow(&d),
        "yolo auto-approves non-sudo, non-sensitive: {d:?}"
    );
}

#[test]
fn sensitive_paths_require_approval_even_in_yolo() {
    let e = engine();
    let d = e.check_tool_call_with_mode("fs", "read ~/.ssh/id_rsa", false, true);
    assert!(!is_allow(&d), "{d:?}");
}

#[test]
fn risk_threshold_controls_auto_approval() {
    // With the threshold at None, any risky-ish call must ask.
    let strict = engine_with(|c| c.auto_approve_risk_level = RiskLevel::None);
    let lenient = engine_with(|c| c.auto_approve_risk_level = RiskLevel::Critical);
    let call = ("terminal", "git push origin main");
    let s = strict.check_tool_call_with_mode(call.0, call.1, false, false);
    let l = lenient.check_tool_call_with_mode(call.0, call.1, false, false);
    assert!(is_ask(&s), "{s:?}");
    assert!(is_allow(&l), "{l:?}");
    if let SecurityDecision::RequireApproval { reason, .. } = s {
        assert!(reason.contains("Risk level"));
    }
}

#[test]
fn engine_mode_flags_feed_check_tool_call() {
    let e = engine();
    let cmd = format!("rm -rf {}", inside("target/scratch-build"));
    assert!(!is_allow(&e.check_tool_call("terminal", &cmd)));
    e.set_yolo_mode(true);
    assert!(is_allow(&e.check_tool_call("terminal", &cmd)));
    e.set_yolo_mode(false);
    assert!(!is_allow(&e.check_tool_call("terminal", &cmd)));
}

#[test]
fn audit_log_records_decisions() {
    let e = engine();
    e.audit("fs", "read a", true, RiskLevel::Low, "approved");
    e.audit("terminal", "sudo x", false, RiskLevel::Critical, "blocked");
    let log = e.get_audit_log(10);
    assert_eq!(log.len(), 2);
}

#[test]
fn output_secrets_are_redacted() {
    let e = engine();
    let out = e.sanitize_output(
        "terminal",
        "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz0123456789",
    );
    assert!(
        !out.contains("sk-abcdefghijklmnopqrstuvwxyz0123456789"),
        "{out}"
    );
}
