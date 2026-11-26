use super::Tool;
use anyhow::{Context, Result};
use serde_json::json;

/// Android system access tool — calls the OpenShield Mobile app services.
///
/// The mobile app runs two embedded HTTP servers:
///   - AndroidBridgeService (port 9877) — files, SMS, contacts, calendar, clipboard, battery, WiFi, apps, device info
///   - OpenShieldAccessibilityService (port 9878) — UI automation: tree, tap, swipe, click, input, key events, screenshots
///
/// This tool tries the HTTP APIs first, then falls back to shell commands.
///
/// Every bridge request carries `Authorization: Bearer <bridge token>` (see
/// "HTTP Client Helpers" below). Fallbacks exec argv directly; model-supplied
/// text never goes through `sh -c`.
///
/// Bridge API (port 9877):
///   GET  /android/files?path=/sdcard/Download
///   GET  /android/files?path=/sdcard/note.txt&action=read
///   POST /android/files/write {path, content}
///   GET  /android/sms?limit=20
///   GET  /android/contacts?query=John
///   GET  /android/calendar?days=7
///   GET  /android/clipboard
///   POST /android/clipboard {text}
///   GET  /android/battery
///   GET  /android/wifi
///   GET  /android/apps
///   POST /android/apps/open {package}
///   GET  /android/device
///
/// Accessibility API (port 9878):
///   GET  /ui/tree?max_nodes=400
///   POST /ui/tap {x, y, duration_ms}
///   POST /ui/swipe {x1, y1, x2, y2, duration_ms}
///   POST /ui/click {target: {resource_id: "save"}}
///   POST /ui/input {text, target: {resource_id: "search"}, clear: true}
///   POST /ui/key {key: "BACK"}
///   GET  /ui/screenshot
pub struct AndroidTool;

impl Tool for AndroidTool {
    fn name(&self) -> &str {
        "android"
    }

    fn description(&self) -> &str {
        "Full Android device control via OpenShield Mobile services. Files, SMS, contacts, calendar, clipboard, battery, apps, UI automation (tap, swipe, click, type), screenshots, key events."
    }

    fn execute(&self, args: &str) -> Result<String> {
        if args.trim().is_empty() {
            return Ok(SELF_HELP.to_string());
        }

        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let category = parts.first().copied().unwrap_or("").trim();
        let operation = parts.get(1).copied().unwrap_or("").trim();
        let rest = parts.get(2).copied().unwrap_or("").trim();

        // Try HTTP API first, fall back to shell

        match category {
            "files" => handle_files(operation, rest),
            "sms" => handle_sms(operation, rest),
            "contacts" => handle_contacts(operation, rest),
            "calendar" => handle_calendar(operation, rest),
            "clipboard" => handle_clipboard(operation, rest),
            "camera" => handle_camera(operation),
            "location" => handle_location(),
            "battery" => handle_battery(),
            "wifi" => handle_wifi(),
            "apps" => handle_apps(operation, rest),
            "notifications" => handle_notifications(),
            "device" => handle_device(operation),
            // UI automation goes to the accessibility service
            "ui" => handle_ui(operation, rest),
            "tap" => handle_ui("tap", format!("{} {}", operation, rest).trim()),
            "swipe" => handle_ui("swipe", format!("{} {}", operation, rest).trim()),
            "click" => handle_ui("click", format!("{} {}", operation, rest).trim()),
            "input" => handle_ui("input", format!("{} {}", operation, rest).trim()),
            "key" => handle_ui("key", operation),
            "tree" => handle_ui("tree", ""),
            "screenshot" => handle_ui("screenshot", ""),
            _ => Ok(format!("Unknown category: {}. {}", category, SELF_HELP)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HTTP Client Helpers
// ═══════════════════════════════════════════════════════════════════════════
//
// Both bridges listen on loopback, which every app on the phone can reach.
// Every request therefore carries `Authorization: Bearer <bridge token>`.
// The token is shared with Pocket (which hosts the bridges) and is read from,
// in order:
//   1. `OPENSHIELD_BRIDGE_TOKEN`
//   2. the file named by `OPENSHIELD_BRIDGE_TOKEN_FILE`
//   3. `~/.config/openshield/bridge.token`
// Token files must not be readable by group or others (0600). When no usable
// token exists the tool never calls the bridge: it reports the error, or uses
// the local fallback (which runs as this process's own user).

const BRIDGE_URL: &str = "http://127.0.0.1:9877";
const A11Y_URL: &str = "http://127.0.0.1:9878";

const BRIDGE_TOKEN_ENV: &str = "OPENSHIELD_BRIDGE_TOKEN";
const BRIDGE_TOKEN_FILE_ENV: &str = "OPENSHIELD_BRIDGE_TOKEN_FILE";
const DEFAULT_BRIDGE_TOKEN_PATH: &str = "~/.config/openshield/bridge.token";
const MIN_BRIDGE_TOKEN_LEN: usize = 32;

fn validate_token(token: &str, source: &str) -> Result<String> {
    let token = token.trim();
    if token.len() < MIN_BRIDGE_TOKEN_LEN {
        anyhow::bail!(
            "Android bridge token from {} is missing or shorter than {} characters",
            source,
            MIN_BRIDGE_TOKEN_LEN
        );
    }
    if token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        anyhow::bail!("Android bridge token from {} contains whitespace", source);
    }
    Ok(token.to_string())
}

fn read_token_file(path: &std::path::Path) -> Result<String> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("Android bridge token file {} not found", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            anyhow::bail!(
                "Android bridge token file {} is readable by other users; chmod 600 it",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    let _ = meta;
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    validate_token(&raw, &path.display().to_string())
}

fn bridge_token() -> Result<String> {
    if let Ok(token) = std::env::var(BRIDGE_TOKEN_ENV)
        && !token.trim().is_empty()
    {
        return validate_token(&token, BRIDGE_TOKEN_ENV);
    }
    let raw = std::env::var(BRIDGE_TOKEN_FILE_ENV)
        .ok()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BRIDGE_TOKEN_PATH.to_string());
    let path = shellexpand::tilde(&raw).into_owned();
    read_token_file(std::path::Path::new(&path))
}

/// One authenticated request to a loopback bridge. No proxy, no redirects.
fn bridge_request(
    base: &str,
    token: &str,
    path: &str,
    body: Option<serde_json::Value>,
    label: &str,
) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let url = format!("{}{}", base, path);
    let req = match body {
        Some(body) => client.post(url).json(&body),
        None => client.get(url),
    };
    let resp = req.bearer_auth(token).send()?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        anyhow::bail!(
            "{} rejected the bridge token ({}). Re-pair Pocket to rotate it.",
            label,
            status
        );
    }
    if !status.is_success() {
        anyhow::bail!("{} error: {}", label, resp.text().unwrap_or_default());
    }
    Ok(resp.text()?)
}

fn bridge_get(path: &str) -> Result<String> {
    let token = bridge_token()?;
    bridge_request(BRIDGE_URL, &token, path, None, "Bridge")
}

fn bridge_post(path: &str, body: serde_json::Value) -> Result<String> {
    let token = bridge_token()?;
    bridge_request(BRIDGE_URL, &token, path, Some(body), "Bridge")
}

fn a11y_get(path: &str) -> Result<String> {
    let token = bridge_token()?;
    bridge_request(A11Y_URL, &token, path, None, "Accessibility service")
}

fn a11y_post(path: &str, body: serde_json::Value) -> Result<String> {
    let token = bridge_token()?;
    bridge_request(A11Y_URL, &token, path, Some(body), "Accessibility service")
}

// ═══════════════════════════════════════════════════════════════════════════
// Bridge API (port 9877) — Files, SMS, Contacts, Calendar, Device Info
// ═══════════════════════════════════════════════════════════════════════════

fn handle_files(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" | "ls" => {
            let path = if args.is_empty() { "/sdcard" } else { args };
            bridge_get(&format!(
                "/android/files?path={}&action=list",
                url_encode(path)
            ))
            .or_else(|_| run_cmd("ls", &["-la", "--", path], None))
        }
        "read" | "cat" => {
            if args.is_empty() {
                return Ok("Usage: android files read <path>".to_string());
            }
            bridge_get(&format!(
                "/android/files?path={}&action=read",
                url_encode(args)
            ))
            .or_else(|_| run_cmd("cat", &["--", args], None))
        }
        "write" => {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            if parts.len() < 2 {
                return Ok("Usage: android files write <path> <content>".to_string());
            }
            let body = json!({"path": parts[0], "content": parts[1]});
            bridge_post("/android/files/write", body).or_else(|_| {
                std::fs::write(parts[0], format!("{}\n", parts[1]))
                    .with_context(|| format!("Failed to write {}", parts[0]))?;
                Ok(format!("Wrote {}", parts[0]))
            })
        }
        "delete" | "rm" => {
            if args.is_empty() {
                return Ok("Usage: android files delete <path>".to_string());
            }
            run_cmd("rm", &["--", args], None)
        }
        _ => Ok(format!(
            "Unknown files operation: {}. Try: list, read, write, delete",
            operation
        )),
    }
}

fn handle_sms(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" => {
            let limit = args.parse::<usize>().unwrap_or(20);
            bridge_get(&format!("/android/sms?limit={}", limit))
                .or_else(|_| run_shell("termux-sms-list"))
        }
        "send" => {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            if parts.len() < 2 {
                return Ok("Usage: android sms send <number> <text>".to_string());
            }
            let body = json!({"number": parts[0], "text": parts[1]});
            if !valid_sms_recipients(parts[0]) {
                return Ok(format!(
                    "Invalid recipient: {}. Use digits, an optional leading +, and commas.",
                    parts[0]
                ));
            }
            bridge_post("/android/sms/send", body).or_else(|_| {
                // Message text goes on stdin so it is never parsed as options.
                run_cmd("termux-sms-send", &["-n", parts[0]], Some(parts[1]))
            })
        }
        _ => Ok("SMS operations: list [limit], send <number> <text>".to_string()),
    }
}

fn handle_contacts(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" | "search" => {
            let query = if args.is_empty() { "" } else { args };
            bridge_get(&format!("/android/contacts?query={}", url_encode(query)))
                .or_else(|_| run_shell("termux-contact-list"))
        }
        _ => Ok("Contacts operations: list [query]".to_string()),
    }
}

fn handle_calendar(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" => {
            let days = args.parse::<usize>().unwrap_or(7);
            bridge_get(&format!("/android/calendar?days={}", days))
        }
        _ => Ok("Calendar operations: list [days]".to_string()),
    }
}

fn handle_clipboard(operation: &str, args: &str) -> Result<String> {
    match operation {
        "get" => bridge_get("/android/clipboard").or_else(|_| run_shell("termux-clipboard-get")),
        "set" => {
            let body = json!({"text": args});
            bridge_post("/android/clipboard", body)
                .or_else(|_| run_cmd("termux-clipboard-set", &[], Some(args)))
        }
        _ => Ok("Clipboard operations: get, set <text>".to_string()),
    }
}

fn handle_camera(operation: &str) -> Result<String> {
    match operation {
        "capture" | "photo" => run_shell("termux-camera-photo -c 0 /sdcard/Pictures/capture.jpg"),
        "info" => run_shell("termux-camera-info"),
        _ => Ok("Camera operations: capture, info".to_string()),
    }
}

fn handle_location() -> Result<String> {
    bridge_get("/android/location").or_else(|_| run_shell("termux-location"))
}

fn handle_battery() -> Result<String> {
    bridge_get("/android/battery")
        .or_else(|_| {
            run_shell("cat /sys/class/power_supply/battery/capacity 2>/dev/null && cat /sys/class/power_supply/battery/status 2>/dev/null")
        })
}

fn handle_wifi() -> Result<String> {
    bridge_get("/android/wifi").or_else(|_| run_shell("termux-wifi-connectioninfo"))
}

fn handle_apps(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" => bridge_get("/android/apps")
            .or_else(|_| run_shell("pm list packages | sed 's/package://'")),
        "open" | "launch" => {
            if args.is_empty() {
                return Ok("Usage: android apps open <package.name>".to_string());
            }
            if !valid_package_name(args) {
                return Ok(format!("Invalid package name: {}", args));
            }
            let body = json!({"package": args});
            bridge_post("/android/apps/open", body).or_else(|_| {
                let component = format!("{}/.MainActivity", args);
                run_cmd_checked("am", &["start", "-n", &component]).or_else(|_| {
                    run_cmd(
                        "am",
                        &["start", "-a", "android.intent.action.MAIN", "-p", args],
                        None,
                    )
                })
            })
        }
        "screenshot" => run_shell(
            "screencap -p /sdcard/Pictures/screenshot.png && echo /sdcard/Pictures/screenshot.png",
        ),
        "info" => {
            if args.is_empty() {
                return Ok("Usage: android apps info <package.name>".to_string());
            }
            if !valid_package_name(args) {
                return Ok(format!("Invalid package name: {}", args));
            }
            let out = run_cmd("dumpsys", &["package", args], None)?;
            Ok(out.lines().take(50).collect::<Vec<_>>().join("\n"))
        }
        _ => Ok("Apps operations: list, open <package>, screenshot, info <package>".to_string()),
    }
}

fn handle_notifications() -> Result<String> {
    bridge_get("/android/notifications")
        .or_else(|_| run_shell("dumpsys notification | grep 'NotificationRecord' | head -20"))
}

fn handle_device(operation: &str) -> Result<String> {
    match operation {
        "info" => bridge_get("/android/device"),
        "storage" => run_shell("df -h /sdcard /data 2>/dev/null || df -h"),
        "display" => run_shell(
            "dumpsys display | grep -E 'DisplayDeviceInfo|width|height|density' | head -10",
        ),
        _ => Ok("Device operations: info, storage, display".to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Accessibility API (port 9878) — UI Automation
// ═══════════════════════════════════════════════════════════════════════════

fn handle_ui(operation: &str, args: &str) -> Result<String> {
    match operation {
        "tree" => {
            let max_nodes = args.parse::<usize>().unwrap_or(400);
            a11y_get(&format!("/ui/tree?max_nodes={}", max_nodes))
        }
        "tap" => {
            let parts: Vec<&str> = args.split_whitespace().collect();
            if parts.len() < 2 {
                return Ok("Usage: android tap <x> <y> [duration_ms]".to_string());
            }
            let x: f32 = parts[0].parse().unwrap_or(0.0);
            let y: f32 = parts[1].parse().unwrap_or(0.0);
            let duration: u64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);
            let body = json!({"x": x, "y": y, "duration_ms": duration});
            a11y_post("/ui/tap", body)
        }
        "swipe" => {
            let parts: Vec<&str> = args.split_whitespace().collect();
            if parts.len() < 4 {
                return Ok("Usage: android swipe <x1> <y1> <x2> <y2> [duration_ms]".to_string());
            }
            let body = json!({
                "x1": parts[0].parse::<f32>().unwrap_or(0.0),
                "y1": parts[1].parse::<f32>().unwrap_or(0.0),
                "x2": parts[2].parse::<f32>().unwrap_or(0.0),
                "y2": parts[3].parse::<f32>().unwrap_or(0.0),
                "duration_ms": parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(300u64)
            });
            a11y_post("/ui/swipe", body)
        }
        "click" => {
            if args.is_empty() {
                return Ok("Usage: android click <resource_id|text|content_desc>".to_string());
            }
            // Try to parse as explicit selector: resource_id=foo or text=bar
            let target = if args.contains('=') {
                let mut map = serde_json::Map::new();
                for pair in args.split(',') {
                    let kv: Vec<&str> = pair.splitn(2, '=').collect();
                    if kv.len() == 2 {
                        map.insert(kv[0].trim().to_string(), json!(kv[1].trim()));
                    }
                }
                json!(map)
            } else {
                // Default: try as resource_id, then text, then content_desc
                json!({"any_text_contains": args})
            };
            let body = json!({"target": target});
            a11y_post("/ui/click", body)
        }
        "input" => {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            if parts.is_empty() {
                return Ok("Usage: android input <text> [target_selector]".to_string());
            }
            let text = parts[0];
            let target = parts.get(1).map(|s| {
                if s.contains('=') {
                    let mut map = serde_json::Map::new();
                    for pair in s.split(',') {
                        let kv: Vec<&str> = pair.splitn(2, '=').collect();
                        if kv.len() == 2 {
                            map.insert(kv[0].trim().to_string(), json!(kv[1].trim()));
                        }
                    }
                    json!(map)
                } else {
                    json!({"resource_id": s.trim()})
                }
            });
            let mut body = json!({"text": text, "clear": true});
            if let Some(t) = target {
                body.as_object_mut()
                    .unwrap()
                    .insert("target".to_string(), t);
            }
            a11y_post("/ui/input", body)
        }
        "key" => {
            if args.is_empty() {
                return Ok("Usage: android key <BACK|HOME|RECENTS|NOTIFICATIONS|POWER>".to_string());
            }
            let body = json!({"key": args.to_uppercase()});
            a11y_post("/ui/key", body)
        }
        "screenshot" => a11y_get("/ui/screenshot"),
        _ => Ok(format!(
            "Unknown UI operation: {}. Try: tree, tap, swipe, click, input, key, screenshot",
            operation
        )),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Shell Fallbacks
// ═══════════════════════════════════════════════════════════════════════════

/// Runs a fixed command string through `sh -c`. Only call this with string
/// literals. Never build `cmd` from model or user input; use [`run_cmd`].
fn run_shell(cmd: &'static str) -> Result<String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .with_context(|| format!("Failed to execute: {}", cmd))?;
    Ok(format_output(&output))
}

/// Runs `program` with an argv (no shell). Optional `stdin` is written to the
/// child, which keeps free text out of option parsing.
fn run_cmd(program: &str, args: &[&str], stdin: Option<&str>) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to execute: {}", program))?;
    if let Some(input) = stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        pipe.write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    Ok(format_output(&output))
}

/// Like [`run_cmd`] without stdin, but fails on a non-zero exit status.
fn run_cmd_checked(program: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("Failed to execute: {}", program))?;
    if !output.status.success() {
        anyhow::bail!("{} exited with {}", program, output.status);
    }
    Ok(format_output(&output))
}

fn format_output(output: &std::process::Output) -> String {
    let mut result = String::new();
    if !output.stdout.is_empty() {
        result.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() && result.trim().is_empty() {
        result.push_str(&format!(
            "[stderr]: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    result
}

/// Java package name: dot-separated segments of `[A-Za-z][A-Za-z0-9_]*`,
/// at least two segments.
fn valid_package_name(s: &str) -> bool {
    let segments: Vec<&str> = s.split('.').collect();
    s.len() <= 255
        && segments.len() >= 2
        && segments.iter().all(|seg| {
            let mut chars = seg.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Comma-separated phone numbers: digits with an optional leading `+`.
fn valid_sms_recipients(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.split(',').all(|n| {
            let digits = n.strip_prefix('+').unwrap_or(n);
            !digits.is_empty() && digits.len() <= 20 && digits.chars().all(|c| c.is_ascii_digit())
        })
}

fn url_encode(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('+', "%2B")
        .replace('&', "%26")
}

const SELF_HELP: &str = r#"Android device control tool.

Requires OpenShield Mobile app running with services enabled.
Bridge calls need the bridge token (OPENSHIELD_BRIDGE_TOKEN or
~/.config/openshield/bridge.token, mode 0600) that Pocket writes when paired.

Data/API (port 9877):
  files list [path]          — List files
  files read <path>          — Read file
  files write <path> <text>  — Write file
  files delete <path>        — Delete file
  sms list [limit]           — List SMS
  contacts list [query]      — Search contacts
  calendar list [days]       — List events
  clipboard get              — Get clipboard
  clipboard set <text>       — Set clipboard
  battery                    — Battery status
  wifi                       — WiFi info
  apps list                  — List installed apps
  apps open <package>        — Open app
  device info                — Device info

UI Automation (port 9878) — requires Accessibility Service:
  tree                       — Capture UI tree
  tap <x> <y>                — Tap screen coordinates
  swipe <x1> <y1> <x2> <y2> — Swipe gesture
  click <selector>           — Click by resource_id/text/content_desc
  input <text> [selector]    — Type into field
  key <BACK|HOME|RECENTS>    — Press system key
  screenshot                 — Capture screen

Examples:
  android files list /sdcard/Download
  android tap 500 1000
  android swipe 540 1800 540 600
  android click resource_id=save_button
  android click text=Submit
  android input "Hello world" resource_id=message_field
  android key BACK
  android tree
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_android_help() {
        let tool = AndroidTool;
        let result = tool.execute("").unwrap();
        assert!(result.contains("Android device control"));
    }

    #[test]
    fn test_android_unknown_category() {
        let tool = AndroidTool;
        let result = tool.execute("foo bar").unwrap();
        assert!(result.contains("Unknown category"));
    }

    #[test]
    fn package_names_are_validated() {
        assert!(valid_package_name("com.openshield.pocket"));
        assert!(valid_package_name("org.fdroid.fdroid"));
        assert!(!valid_package_name("pocket"));
        assert!(!valid_package_name("com.x; reboot"));
        assert!(!valid_package_name("com.x/.Evil"));
        assert!(!valid_package_name("-n com.x"));
        assert!(!valid_package_name("com..x"));
    }

    #[test]
    fn sms_recipients_are_validated() {
        assert!(valid_sms_recipients("+15551234567"));
        assert!(valid_sms_recipients("5551234567,+15557654321"));
        assert!(!valid_sms_recipients("-n"));
        assert!(!valid_sms_recipients("555;id"));
        assert!(!valid_sms_recipients(""));
    }

    #[cfg(unix)]
    #[test]
    fn token_file_must_be_private_and_long_enough() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("os-bridge-tok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bridge.token");
        let token = "a".repeat(48);
        std::fs::write(&path, format!("{}\n", token)).unwrap();

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            read_token_file(&path).is_err(),
            "world-readable token must be refused"
        );

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_token_file(&path).unwrap(), token);

        std::fs::write(&path, "short").unwrap();
        assert!(
            read_token_file(&path).is_err(),
            "short token must be refused"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A one-shot fake bridge that records the request head and replies 200
    /// only when the expected bearer token is present.
    fn fake_bridge(expected: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let want = format!("authorization: bearer {}", expected).to_lowercase();
            let ok = head.to_lowercase().lines().any(|l| l.trim() == want);
            let reply = if ok {
                "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok"
            } else {
                "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            };
            stream.write_all(reply.as_bytes()).unwrap();
            head
        });
        (base, handle)
    }

    #[test]
    fn bridge_request_sends_bearer_token() {
        const TOKEN: &str = "0123456789abcdef0123456789abcdef";
        let (base, handle) = fake_bridge(TOKEN);
        let out = bridge_request(&base, TOKEN, "/android/battery", None, "Bridge").unwrap();
        assert_eq!(out, "ok");
        handle.join().unwrap();
    }

    #[test]
    fn bridge_request_reports_rejected_token() {
        const TOKEN: &str = "0123456789abcdef0123456789abcdef";
        let (base, handle) = fake_bridge(TOKEN);
        let err = bridge_request(
            &base,
            "ffffffffffffffffffffffffffffffff",
            "/ui/tree",
            None,
            "Bridge",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("rejected the bridge token"), "{}", err);
        handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn fallbacks_do_not_interpret_shell_metacharacters() {
        let dir = std::env::temp_dir().join(format!("os-bridge-inj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("pwned");
        let evil = format!("/nonexistent'; touch {}; echo '", marker.display());
        let _ = run_cmd("cat", &["--", &evil], None);
        let _ = run_cmd("ls", &["-la", "--", &evil], None);
        assert!(!marker.exists(), "argv exec must not run injected commands");
        std::fs::remove_dir_all(&dir).ok();
    }
}
