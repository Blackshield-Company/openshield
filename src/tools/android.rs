use super::Tool;
use anyhow::{Context, Result};
use serde_json::json;

/// Android system access tool — calls the OpenShark Mobile app services.
///
/// The mobile app runs two embedded HTTP servers:
///   - AndroidBridgeService (port 9877) — files, SMS, contacts, calendar, clipboard, battery, WiFi, apps, device info
///   - OpenSharkAccessibilityService (port 9878) — UI automation: tree, tap, swipe, click, input, key events, screenshots
///
/// This tool tries the HTTP APIs first, then falls back to shell commands.
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
        "Full Android device control via OpenShark Mobile services. Files, SMS, contacts, calendar, clipboard, battery, apps, UI automation (tap, swipe, click, type), screenshots, key events."
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
        let result = match category {
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
            "tap" => handle_ui("tap", &format!("{} {}", operation, rest).trim()),
            "swipe" => handle_ui("swipe", &format!("{} {}", operation, rest).trim()),
            "click" => handle_ui("click", &format!("{} {}", operation, rest).trim()),
            "input" => handle_ui("input", &format!("{} {}", operation, rest).trim()),
            "key" => handle_ui("key", operation),
            "tree" => handle_ui("tree", ""),
            "screenshot" => handle_ui("screenshot", ""),
            _ => Ok(format!("Unknown category: {}. {}", category, SELF_HELP)),
        };

        result
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HTTP Client Helpers
// ═══════════════════════════════════════════════════════════════════════════

const BRIDGE_URL: &str = "http://127.0.0.1:9877";
const A11Y_URL: &str = "http://127.0.0.1:9878";

fn bridge_get(path: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let resp = client.get(&format!("{}{}", BRIDGE_URL, path)).send()?;
    if !resp.status().is_success() {
        anyhow::bail!("Bridge error: {}", resp.text().unwrap_or_default());
    }
    Ok(resp.text()?)
}

fn bridge_post(path: &str, body: serde_json::Value) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let resp = client.post(&format!("{}{}", BRIDGE_URL, path))
        .json(&body)
        .send()?;
    if !resp.status().is_success() {
        anyhow::bail!("Bridge error: {}", resp.text().unwrap_or_default());
    }
    Ok(resp.text()?)
}

fn a11y_get(path: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let resp = client.get(&format!("{}{}", A11Y_URL, path)).send()?;
    if !resp.status().is_success() {
        anyhow::bail!("Accessibility service error: {}", resp.text().unwrap_or_default());
    }
    Ok(resp.text()?)
}

fn a11y_post(path: &str, body: serde_json::Value) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let resp = client.post(&format!("{}{}", A11Y_URL, path))
        .json(&body)
        .send()?;
    if !resp.status().is_success() {
        anyhow::bail!("Accessibility service error: {}", resp.text().unwrap_or_default());
    }
    Ok(resp.text()?)
}

// ═══════════════════════════════════════════════════════════════════════════
// Bridge API (port 9877) — Files, SMS, Contacts, Calendar, Device Info
// ═══════════════════════════════════════════════════════════════════════════

fn handle_files(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" | "ls" => {
            let path = if args.is_empty() { "/sdcard" } else { args };
            bridge_get(&format!("/android/files?path={}&action=list", url_encode(path)))
                .or_else(|_| run_shell(&format!("ls -la '{}'", shell_escape(path))))
        }
        "read" | "cat" => {
            if args.is_empty() {
                return Ok("Usage: android files read <path>".to_string());
            }
            bridge_get(&format!("/android/files?path={}&action=read", url_encode(args)))
                .or_else(|_| run_shell(&format!("cat '{}'", shell_escape(args))))
        }
        "write" => {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            if parts.len() < 2 {
                return Ok("Usage: android files write <path> <content>".to_string());
            }
            let body = json!({"path": parts[0], "content": parts[1]});
            bridge_post("/android/files/write", body)
                .or_else(|_| run_shell(&format!("echo '{}' > '{}'", shell_escape(parts[1]), shell_escape(parts[0]))))
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
            bridge_post("/android/sms/send", body)
                .or_else(|_| run_shell(&format!("termux-sms-send -n {} '{}'", parts[0], shell_escape(parts[1]))))
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
        "get" => bridge_get("/android/clipboard")
            .or_else(|_| run_shell("termux-clipboard-get")),
        "set" => {
            let body = json!({"text": args});
            bridge_post("/android/clipboard", body)
                .or_else(|_| run_shell(&format!("termux-clipboard-set '{}'", shell_escape(args))))
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
    bridge_get("/android/location")
        .or_else(|_| run_shell("termux-location"))
}

fn handle_battery() -> Result<String> {
    bridge_get("/android/battery")
        .or_else(|_| {
            run_shell("cat /sys/class/power_supply/battery/capacity 2>/dev/null && cat /sys/class/power_supply/battery/status 2>/dev/null")
        })
}

fn handle_wifi() -> Result<String> {
    bridge_get("/android/wifi")
        .or_else(|_| run_shell("termux-wifi-connectioninfo"))
}

fn handle_apps(operation: &str, args: &str) -> Result<String> {
    match operation {
        "list" => bridge_get("/android/apps")
            .or_else(|_| run_shell("pm list packages | sed 's/package://'")),
        "open" | "launch" => {
            if args.is_empty() {
                return Ok("Usage: android apps open <package.name>".to_string());
            }
            let body = json!({"package": args});
            bridge_post("/android/apps/open", body)
                .or_else(|_| run_shell(&format!("am start -n {}/.MainActivity 2>/dev/null || am start -a android.intent.action.MAIN -p {}", args, args)))
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

fn handle_notifications() -> Result<String> {
    bridge_get("/android/notifications")
        .or_else(|_| run_shell("dumpsys notification | grep 'NotificationRecord' | head -20"))
}

fn handle_device(operation: &str) -> Result<String> {
    match operation {
        "info" => bridge_get("/android/device"),
        "storage" => run_shell("df -h /sdcard /data 2>/dev/null || df -h"),
        "display" => run_shell("dumpsys display | grep -E 'DisplayDeviceInfo|width|height|density' | head -10"),
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
                body.as_object_mut().unwrap().insert("target".to_string(), t);
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

fn run_shell(cmd: &str) -> Result<String> {
    let output = std::process::Command::new("sh")
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
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`")
}

fn url_encode(s: &str) -> String {
    s.replace(' ', "%20").replace('+', "%2B").replace('&', "%26")
}

const SELF_HELP: &str = r#"Android device control tool.

Requires OpenShark Mobile app running with services enabled.

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
}
