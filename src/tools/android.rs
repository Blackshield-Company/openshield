use super::Tool;
use anyhow::{Context, Result};
use std::process::Command;

/// Android system access tool — wraps kimiclaw-cli or Termux:API
/// to provide full device access like KimiClaw.
///
/// Supported operations:
/// - `files list <path>` — List files on sdcard
/// - `files read <path>` — Read file content
/// - `files write <path> <content>` — Write file
/// - `files delete <path>` — Delete file
/// - `sms list [limit]` — List SMS messages
/// - `sms send <number> <text>` — Send SMS
/// - `contacts list [query]` — Search contacts
/// - `calendar list [days]` — List calendar events
/// - `clipboard get` — Get clipboard content
/// - `clipboard set <text>` — Set clipboard
/// - `camera capture` — Take a photo
/// - `location` — Get current location
/// - `battery` — Get battery status
/// - `wifi` — Get WiFi info
/// - `apps list` — List installed apps
/// - `apps open <package>` — Open app by package name
/// - `apps screenshot` — Capture screenshot
/// - `notifications` — List recent notifications
/// - `device info` — Get device information
pub struct AndroidTool;

impl Tool for AndroidTool {
    fn name(&self) -> &str {
        "android"
    }

    fn description(&self) -> &str {
        "Access Android device APIs: files, SMS, contacts, calendar, clipboard, camera, location, battery, apps, notifications, device info. Use 'android <operation> <args>' syntax."
    }

    fn execute(&self, args: &str) -> Result<String> {
        if args.trim().is_empty() {
            return Ok(SELF_HELP.to_string());
        }

        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let category = parts.first().copied().unwrap_or("").trim();
        let operation = parts.get(1).copied().unwrap_or("").trim();
        let rest = parts.get(2).copied().unwrap_or("").trim();

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
            _ => Ok(format!("Unknown category: {}. {}", category, SELF_HELP)),
        }
    }
}

// ── Files ────────────────────────────────────────────────────────────────
fn handle_files(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" | "ls" => {
            let path = if args.is_empty() { "/sdcard" } else { args };
            run_kimiclaw(&["sdcard", "list", path])
                .or_else(|_| run_termux_api("storage-list"))
        }
        "read" | "cat" => {
            if args.is_empty() {
                return Ok("Usage: android files read <path>".to_string());
            }
            run_kimiclaw(&["sdcard", "read", args])
                .or_else(|_| run_shell(&format!("cat '{}'", shell_escape(args))))
        }
        "write" => {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            if parts.len() < 2 {
                return Ok("Usage: android files write <path> <content>".to_string());
            }
            let path = parts[0];
            let content = parts[1];
            run_shell(&format!("echo '{}' > '{}'", shell_escape(content), shell_escape(path)))
        }
        "delete" | "rm" => {
            if args.is_empty() {
                return Ok("Usage: android files delete <path>".to_string());
            }
            run_shell(&format!("rm '{}'", shell_escape(args)))
        }
        _ => Ok(format!(
            "Unknown files operation: {}. Try: list, read, write, delete",
            operation
        )),
    }
}

// ── SMS ──────────────────────────────────────────────────────────────────
fn handle_sms(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" => {
            let limit = args.parse::<usize>().unwrap_or(20);
            run_kimiclaw(&["sms", "list", &limit.to_string()])
                .or_else(|_| run_termux_api(&format!("sms-list --limit {}", limit)))
        }
        "send" => {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            if parts.len() < 2 {
                return Ok("Usage: android sms send <number> <text>".to_string());
            }
            run_termux_api(&format!(
                "sms-send --number {} --text '{}'",
                parts[0],
                shell_escape(parts[1])
            ))
        }
        _ => Ok("SMS operations: list [limit], send <number> <text>".to_string()),
    }
}

// ── Contacts ─────────────────────────────────────────────────────────────
fn handle_contacts(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" | "search" => {
            let query = if args.is_empty() { "" } else { args };
            run_kimiclaw(&["contact", "search", query])
                .or_else(|_| run_termux_api("contact-list"))
        }
        _ => Ok("Contacts operations: list [query]".to_string()),
    }
}

// ── Calendar ─────────────────────────────────────────────────────────────
fn handle_calendar(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" => {
            let days = args.parse::<usize>().unwrap_or(7);
            run_kimiclaw(&["calendar", "list", &days.to_string()])
        }
        _ => Ok("Calendar operations: list [days]".to_string()),
    }
}

// ── Clipboard ────────────────────────────────────────────────────────────
fn handle_clipboard(operation: &str, args: &str) -> Result<String> {
    match operation {
        "get" => run_termux_api("clipboard-get"),
        "set" => run_termux_api(&format!("clipboard-set '{}'", shell_escape(args))),
        _ => Ok("Clipboard operations: get, set <text>".to_string()),
    }
}

// ── Camera ───────────────────────────────────────────────────────────────
fn handle_camera(operation: &str) -> Result<String> {
    match operation {
        "capture" | "photo" => run_termux_api("camera-photo -c 0 /sdcard/Pictures/capture.jpg"),
        "info" => run_shell("termux-camera-info"),
        _ => Ok("Camera operations: capture, info".to_string()),
    }
}

// ── Location ─────────────────────────────────────────────────────────────
fn handle_location() -> Result<String> {
    run_termux_api("location -r last").or_else(|_| run_shell("termux-location"))
}

// ── Battery ──────────────────────────────────────────────────────────────
fn handle_battery() -> Result<String> {
    run_shell("termux-battery-status").or_else(|_| {
        // Fallback: read from Android sysfs
        run_shell("cat /sys/class/power_supply/battery/capacity 2>/dev/null && cat /sys/class/power_supply/battery/status 2>/dev/null")
    })
}

// ── WiFi ─────────────────────────────────────────────────────────────────
fn handle_wifi() -> Result<String> {
    run_shell("termux-wifi-connectioninfo")
}

// ── Apps ─────────────────────────────────────────────────────────────────
fn handle_apps(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" => run_shell("pm list packages | sed 's/package://'"),
        "open" | "launch" => {
            if args.is_empty() {
                return Ok("Usage: android apps open <package.name>".to_string());
            }
            run_shell(&format!("am start -n {}/.MainActivity 2>/dev/null || am start -a android.intent.action.MAIN -p {}", args, args))
        }
        "screenshot" => run_shell("screencap -p /sdcard/Pictures/screenshot.png && echo /sdcard/Pictures/screenshot.png"),
        "info" => {
            if args.is_empty() {
                return Ok("Usage: android apps info <package.name>".to_string());
            }
            run_shell(&format!("dumpsys package {} | head -50", args))
        }
        _ => Ok("Apps operations: list, open <package>, screenshot, info <package>".to_string()),
    }
}

// ── Notifications ────────────────────────────────────────────────────────
fn handle_notifications() -> Result<String> {
    run_shell("dumpsys notification | grep 'NotificationRecord' | head -20")
}

// ── Device ───────────────────────────────────────────────────────────────
fn handle_device(operation: &str) -> Result<String> {
    match operation {
        "info" => {
            let mut result = String::new();
            result.push_str("=== Device Info ===\n");
            if let Ok(v) = run_shell("getprop ro.product.model") { result.push_str(&format!("Model: {}\n", v.trim())); }
            if let Ok(v) = run_shell("getprop ro.product.manufacturer") { result.push_str(&format!("Manufacturer: {}\n", v.trim())); }
            if let Ok(v) = run_shell("getprop ro.build.version.release") { result.push_str(&format!("Android: {}\n", v.trim())); }
            if let Ok(v) = run_shell("getprop ro.build.version.sdk") { result.push_str(&format!("SDK: {}\n", v.trim())); }
            if let Ok(v) = run_shell("uname -m") { result.push_str(&format!("Arch: {}\n", v.trim())); }
            if let Ok(v) = run_shell("cat /proc/cpuinfo | grep 'Processor' | head -1") { result.push_str(&format!("CPU: {}\n", v.trim())); }
            if let Ok(v) = run_shell("cat /proc/meminfo | grep 'MemTotal'") { result.push_str(&format!("{}\n", v.trim())); }
            Ok(result)
        }
        "storage" => run_shell("df -h /sdcard /data 2>/dev/null || df -h"),
        "display" => run_shell("dumpsys display | grep -E 'DisplayDeviceInfo|width|height|density' | head -10"),
        _ => Ok("Device operations: info, storage, display".to_string()),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Try kimiclaw-cli first (full Android API access like KimiClaw)
fn run_kimiclaw(args: &[&str]) -> Result<String> {
    let output = Command::new("kimiclaw-cli")
        .args(args)
        .output()
        .context("kimiclaw-cli not found — install KimiClaw for full Android access")?;

    if !output.status.success() {
        anyhow::bail!(
            "kimiclaw-cli error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Fallback to termux-api (basic Android access)
fn run_termux_api(args: &str) -> Result<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("termux-api {} 2>/dev/null || termux-{} 2>/dev/null", args, args.split_whitespace().next().unwrap_or(args)))
        .output()
        .context("termux-api not found")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() && !output.status.success() {
        anyhow::bail!("termux-api error: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(stdout.to_string())
}

/// Fallback to raw shell commands
fn run_shell(cmd: &str) -> Result<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .with_context(|| format!("Failed to execute: {}", cmd))?;

    let mut result = String::new();
    if !output.stdout.is_empty() {
        result.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() && result.trim().is_empty() {
        result.push_str(&format!("[stderr]: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(result)
}

fn shell_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "'\"'\"'").replace('"', "\\\"")
}

const SELF_HELP: &str = r#"Android system access tool.

Categories:
  files      — list, read, write, delete (sdcard & system paths)
  sms        — list, send (SMS messages)
  contacts   — list, search (phone contacts)
  calendar   — list (calendar events)
  clipboard  — get, set (device clipboard)
  camera     — capture (take photo)
  location   — get GPS/location
  battery    — battery status
  wifi       — WiFi connection info
  apps       — list, open, screenshot, info (installed apps)
  notifications — list recent notifications
  device     — info, storage, display (system info)

Examples:
  android files list /sdcard/Download
  android files read /sdcard/Download/note.txt
  android sms list 10
  android contacts list John
  android clipboard get
  android clipboard set "Hello from OpenShark"
  android apps list
  android apps open com.termux
  android device info
  android battery

For full access, install KimiClaw alongside OpenShark.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_android_help() {
        let tool = AndroidTool;
        let result = tool.execute("").unwrap();
        assert!(result.contains("Android system access"));
    }

    #[test]
    fn test_android_unknown_category() {
        let tool = AndroidTool;
        let result = tool.execute("foo bar").unwrap();
        assert!(result.contains("Unknown category"));
    }
}
