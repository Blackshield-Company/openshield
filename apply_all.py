#!/usr/bin/env python3
"""Apply all changes at once: parse_json_tool_format generalization,
ToolCall args parsing fix, and synthesis timeouts."""
import sys

PATH = '/home/synth/projects/openshark/src/tui/mod.rs'
with open(PATH, 'r') as f:
    content = f.read()

changes = 0
errors = []

# ============================================================
# CHANGE 1: parse_json_tool_format + helpers
# ============================================================
old = '''/// Parse JSON tool format: tool_name:0>{"args":"value"} or tool_name:0>{"query":"value"}
fn parse_json_tool_format(rest: &str) -> Option<(String, String)> {
    // Find the first ':' followed by a digit and '>'
    let colon_pos = rest.find(':')?;
    let after_colon = &rest[colon_pos + 1..];
    if after_colon.is_empty() || !after_colon.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    // Find the '>'
    let gt_pos = after_colon.find('>')?;
    if !after_colon[gt_pos..].starts_with(">{") {
        return None;
    }

    let tool_name = rest[..colon_pos].trim().to_string();
    let json_str = &after_colon[gt_pos + 1..]; // after '>'

    // Parse JSON to extract "args" or "query" value
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(args) = v.get("args").and_then(|a| a.as_str()) {
            return Some((tool_name, args.to_string()));
        }
        if let Some(args) = v.get("query").and_then(|a| a.as_str()) {
            return Some((tool_name, args.to_string()));
        }
        // Fallback: collect all string values from the JSON object
        if let Some(obj) = v.as_object() {
            let parts: Vec<&str> = obj.values().filter_map(|v| v.as_str()).collect();
            if !parts.is_empty() {
                return Some((tool_name, parts.join(" ")));
            }
        }
    }
    None
}'''

new = '''/// Parse JSON tool format: supports two forms:
/// 1. tool_name {"key": "value", ...}    (bare JSON)
/// 2. tool_name:0>{"key": "value", ...}  (numeric-indexed)
fn parse_json_tool_format(rest: &str) -> Option<(String, String)> {
    // Try bare JSON format first: tool_name {"key": "value"}
    if let Some(space_pos) = rest.find(|c: char| c == '{' || c == ':') {
        if rest.as_bytes().get(space_pos) == Some(&b'{') {
            let tool_name = rest[..space_pos].trim().to_string();
            if tool_name.is_empty() {
                return None;
            }
            let json_str = find_balanced_json(&rest[space_pos..])?;
            return extract_args_from_json(json_str, &tool_name);
        }
    }

    // Try numeric-indexed format: tool_name:0>{"key": "value"}
    let colon_pos = rest.find(':')?;
    let after_colon = &rest[colon_pos + 1..];
    if after_colon.is_empty() || !after_colon.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let gt_pos = after_colon.find('>')?;
    if !after_colon[gt_pos..].starts_with(">{") {
        return None;
    }

    let tool_name = rest[..colon_pos].trim().to_string();
    let json_str = find_balanced_json(&after_colon[gt_pos + 1..])?;
    extract_args_from_json(json_str, &tool_name)
}

/// Find balanced JSON string starting with '{' or '['. Returns the substring
/// from the opening brace through the matching closing brace.
fn find_balanced_json(s: &str) -> Option<&str> {
    if s.is_empty() {
        return None;
    }
    let first_char = s.chars().next()?;
    let (open, close) = match first_char {
        '{' => ('{', '}'),
        '[' => ('[', ']'),
        _ => return None,
    };
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if !in_string {
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
        }
    }
    None
}

/// Extract args from a parsed JSON object string.
/// Maps JSON fields to space-separated args that each tool's `execute` method expects.
/// Supports all native tools plus a generic fallback for unknown/MCP tools.
fn extract_args_from_json(json_str: &str, tool_name: &str) -> Option<(String, String)> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
        // Common single-field shortcuts
        if let Some(args) = v.get("args").and_then(|a| a.as_str()) {
            return Some((tool_name.to_string(), args.to_string()));
        }
        if let Some(args) = v.get("query").and_then(|a| a.as_str()) {
            return Some((tool_name.to_string(), args.to_string()));
        }

        // Per-tool field extraction order
        let fields: &[&str] = match tool_name {
            "fs" => &["operation", "path", "content"],
            "git" => &["command", "args"],
            "search" => &["query", "path"],
            "grep" => &["pattern", "path"],
            "terminal" => &["command"],
            "edit" => &["file", "old_string", "new_string"],
            "test" => &["path", "framework"],
            "refactor" => &["operation", "file", "line", "column", "new_name"],
            "lsp" => &["command", "file", "line", "column"],
            _ => return extract_generic_args(v, tool_name),
        };

        let mut parts: Vec<String> = Vec::with_capacity(fields.len());
        for field in fields {
            if let Some(val) = v.get(*field) {
                match val {
                    serde_json::Value::String(s) => parts.push(s.clone()),
                    serde_json::Value::Number(n) => parts.push(n.to_string()),
                    _ => {}
                }
            }
        }

        if parts.is_empty() {
            return None;
        }

        // Special formatting for tools that need non-space-separated args
        // edit: infer "replace" or "read" command, format with newlines for replace_in_file
        if tool_name == "edit" && !parts.is_empty() {
            let file = parts.get(0).cloned().unwrap_or_default();
            if file.is_empty() {
                return None;
            }
            let old_str = parts.get(1).cloned().unwrap_or_default();
            let new_str = parts.get(2).cloned().unwrap_or_default();
            if !old_str.is_empty() {
                return Some((tool_name.to_string(), format!("replace {}\\n{}\\n---\\n{}", file, old_str, new_str)));
            }
            return Some((tool_name.to_string(), format!("read {}", file)));
        }
        // test: prepend "run" command
        if tool_name == "test" && !parts.is_empty() {
            let mut reordered = vec!["run".to_string()];
            reordered.extend(parts);
            return Some((tool_name.to_string(), reordered.join(" ")));
        }

        Some((tool_name.to_string(), parts.join(" ")))
    } else {
        None
    }
}

/// Generic fallback for unknown/MCP tools: collect all string/number values.
fn extract_generic_args(v: serde_json::Value, tool_name: &str) -> Option<(String, String)> {
    if let Some(obj) = v.as_object() {
        let parts: Vec<String> = obj
            .values()
            .filter_map(|val| match val {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect();
        if !parts.is_empty() {
            return Some((tool_name.to_string(), parts.join(" ")));
        }
    }
    None
}'''

if old in content:
    content = content.replace(old, new)
    changes += 1
    print("OK: CHANGE 1 - parse_json_tool_format generalized")
else:
    errors.append("CHANGE 1: old parse_json_tool_format not found")

# ============================================================
# CHANGE 2: ToolCall args parsing
# ============================================================
old2 = '''                        // Parse the JSON arguments to extract the actual args string for our tools
                        let tool_args = match serde_json::from_str::<serde_json::Value>(&args) {
                            Ok(v) => {
                                // For tools with a single "command" or "args" field, extract it
                                if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
                                    cmd.to_string()
                                } else if let Some(a) = v.get("args").and_then(|a| a.as_str()) {
                                    a.to_string()
                                } else {
                                    // Otherwise pass the full JSON as args
                                    args.clone()
                                }
                            }
                            Err(_) => args.clone(),
                        };'''

new2 = '''                        let tool_args = extract_args_from_json(&args, &name)
                            .map(|(_, extracted)| extracted)
                            .unwrap_or_else(|| args.clone());'''

if old2 in content:
    content = content.replace(old2, new2, 1)
    changes += 1
    print("OK: CHANGE 2 - ToolCall args parsing")
else:
    errors.append("CHANGE 2: old ToolCall args parsing not found")

# ============================================================
# CHANGE 3: Wrap synthesis calls with timeout
# ============================================================

# 3a: Wrap follow_up call
old3a = '    match provider.chat_stream(follow_up).await {'
new3a = '    match tokio::time::timeout(Duration::from_secs(120), provider.chat_stream(follow_up)).await {'
if old3a in content:
    content = content.replace(old3a, new3a, 1)
    changes += 1
    print("OK: CHANGE 3a - follow_up timeout wrap")

# 3b: Wrap first retry_req call + convert Ok
old3b = '                match provider.chat_stream(retry_req).await {'
new3b = '                match tokio::time::timeout(Duration::from_secs(120), provider.chat_stream(retry_req)).await {'
while old3b in content:
    content = content.replace(old3b, new3b, 1)
    changes += 1
    print("OK: CHANGE 3b/c - retry_req timeout wrap")

# 3d: Convert Ok in follow_up
old3d = '        Ok((follow_chunks, _metrics)) => {'
new3d = '        Ok(Ok((follow_chunks, _metrics))) => {'
if old3d in content:
    content = content.replace(old3d, new3d, 1)
    changes += 1
    print("OK: CHANGE 3d - Ok -> Ok(Ok)")

# 3e: Convert Ok in retry_req
old3e = '                    Ok((retry_chunks, _retry_metrics)) => {'
new3e = '                    Ok(Ok((retry_chunks, _retry_metrics))) => {'
while old3e in content:
    content = content.replace(old3e, new3e, 1)
    changes += 1
    print("OK: CHANGE 3e - retry Ok -> Ok(Ok)")

# 3f: Convert Err in follow_up + add timeout arm
old3f = '''        Err(e) => {
            let _ = tx.send(StreamEvent::Error(format!("Follow-up failed: {}", e)));
            let _ = tx.send(StreamEvent::Done);
        }
    }

    Ok(())
}'''

new3f = '''        Ok(Err(e)) => {
            let _ = tx.send(StreamEvent::Error(format!("Follow-up failed: {}", e)));
            let _ = tx.send(StreamEvent::Done);
        }
        Err(_) => {
            let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));
            let _ = tx.send(StreamEvent::Done);
        }
    }

    Ok(())
}'''

if old3f in content:
    content = content.replace(old3f, new3f, 1)
    changes += 1
    print("OK: CHANGE 3f - follow_up Err -> Ok(Err) + timeout")
else:
    errors.append("CHANGE 3f: follow_up Err pattern not found")

# 3g: Convert ALL remaining Err in retry_req arms
# Search for Err(e) arms that follow timeout-wrapped matches
# These have specific patterns with "Retry follow-up failed"
old3g = '''                    Err(e) => {
                        let _ =
                            tx.send(StreamEvent::Error(format!("Retry follow-up failed: {}", e)));
                        let _ = tx.send(StreamEvent::Done);
                    }'''

new3g = '''                    Ok(Err(e)) => {
                        let _ =
                            tx.send(StreamEvent::Error(format!("Retry follow-up failed: {}", e)));
                        let _ = tx.send(StreamEvent::Done);
                    }
                    Err(_) => {
                        let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));
                        let _ = tx.send(StreamEvent::Done);
                    }'''

count = content.count(old3g)
print(f"Found {count} remaining Err arm(s)")
while old3g in content:
    content = content.replace(old3g, new3g, 1)
    changes += 1
    print("OK: CHANGE 3g - retry Err -> Ok(Err) + timeout")

# ============================================================
# Write output
# ============================================================
if errors:
    print("\nERRORS:")
    for e in errors:
        print(f"  {e}")
    sys.exit(1)

with open(PATH, 'w') as f:
    f.write(content)

print(f"\nAll {changes} changes applied successfully")
