#!/usr/bin/env python3
"""Apply all changes to openshark/src/tui/mod.rs:
1. Replace parse_json_tool_format with generalized version + add helpers
2. Replace ToolCall inline args parsing with extract_args_from_json
3. Wrap synthesis chat_stream calls with tokio::time::timeout(120s)
"""
import sys

with open('/home/synth/projects/openshark/src/tui/mod.rs', 'r') as f:
    content = f.read()

changes_applied = 0

# ============================================================
# CHANGE 1: Replace parse_json_tool_format + add helper functions
# ============================================================
old_ptf = '''/// Parse JSON tool format: tool_name:0>{"args":"value"} or tool_name:0>{"query":"value"}
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

new_ptf = '''/// Parse JSON tool format: supports two forms:
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

if old_ptf in content:
    content = content.replace(old_ptf, new_ptf)
    changes_applied += 1
    print("CHANGE 1 (parse_json_tool_format): OK")
else:
    print("CHANGE 1 (parse_json_tool_format): FAILED - old string not found")
    sys.exit(1)

# ============================================================
# CHANGE 2: Replace ToolCall inline args parsing
# ============================================================
old_tc = '''                        // Parse the JSON arguments to extract the actual args string for our tools
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

new_tc = '''                        let tool_args = extract_args_from_json(&args, &name)
                            .map(|(_, extracted)| extracted)
                            .unwrap_or_else(|| args.clone());'''

if old_tc in content:
    content = content.replace(old_tc, new_tc, 1)
    changes_applied += 1
    print("CHANGE 2 (ToolCall args parsing): OK")
else:
    print("CHANGE 2 (ToolCall args parsing): FAILED - old string not found")
    sys.exit(1)

# ============================================================
# CHANGE 3: Wrap synthesis chat_stream calls with timeout
# ============================================================

# 3a: First follow_up call
old = '    match provider.chat_stream(follow_up).await {'
new = '    match tokio::time::timeout(Duration::from_secs(120), provider.chat_stream(follow_up)).await {'
if old in content:
    content = content.replace(old, new, 1)
    changes_applied += 1
    print("CHANGE 3a (wrap follow_up): OK")
else:
    print("CHANGE 3a: FAILED")

# 3b: First retry_req call
old = '''                match provider.chat_stream(retry_req).await {
                    Ok((retry_chunks, _retry_metrics)) => {'''
new = '''                match tokio::time::timeout(Duration::from_secs(120), provider.chat_stream(retry_req)).await {
                    Ok(Ok((retry_chunks, _retry_metrics))) => {'''
if old in content:
    content = content.replace(old, new, 1)
    changes_applied += 1
    print("CHANGE 3b (wrap retry_req #1): OK")
else:
    print("CHANGE 3b: FAILED")

# 3c: Second retry_req call
old = '''                match provider.chat_stream(retry_req).await {
                    Ok((retry_chunks, _retry_metrics)) => {'''
new = '''                match tokio::time::timeout(Duration::from_secs(120), provider.chat_stream(retry_req)).await {
                    Ok(Ok((retry_chunks, _retry_metrics))) => {'''
if old in content:
    content = content.replace(old, new, 1)
    changes_applied += 1
    print("CHANGE 3c (wrap retry_req #2): OK")
else:
    print("CHANGE 3c: FAILED")

# 3d: Fix follow_up Ok pattern
old = '        Ok((follow_chunks, _metrics)) => {'
new = '        Ok(Ok((follow_chunks, _metrics))) => {'
if old in content:
    content = content.replace(old, new, 1)
    changes_applied += 1
    print("CHANGE 3d (Ok -> Ok(Ok)): OK")
else:
    print("CHANGE 3d: FAILED")

# 3e: Fix follow_up Err -> Ok(Err) + timeout arm
old = '''        Err(e) => {
            let _ = tx.send(StreamEvent::Error(format!("Follow-up failed: {}", e)));
            let _ = tx.send(StreamEvent::Done);
        }'''

new = '''        Ok(Err(e)) => {
            let _ = tx.send(StreamEvent::Error(format!("Follow-up failed: {}", e)));
            let _ = tx.send(StreamEvent::Done);
        }
        Err(_) => {
            let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));
            let _ = tx.send(StreamEvent::Done);
        }'''

if old in content:
    content = content.replace(old, new, 1)
    changes_applied += 1
    print("CHANGE 3e (Err -> Ok(Err) + timeout #1): OK")
else:
    print("CHANGE 3e: FAILED")

# 3f: Fix first retry Err -> Ok(Err) + timeout arm
old = '''                    Err(e) => {
                        let _ =
                            tx.send(StreamEvent::Error(format!("Retry follow-up failed: {}", e)));
                        let _ = tx.send(StreamEvent::Done);
                    }'''

new = '''                    Ok(Err(e)) => {
                        let _ =
                            tx.send(StreamEvent::Error(format!("Retry follow-up failed: {}", e)));
                        let _ = tx.send(StreamEvent::Done);
                    }
                    Err(_) => {
                        let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));
                        let _ = tx.send(StreamEvent::Done);
                    }'''

count = content.count(old)
print(f"Retry Err pattern found {count} times")
while old in content:
    content = content.replace(old, new, 1)
    changes_applied += 1
    print(f"CHANGE 3f (Err -> Ok(Err) + timeout retry): OK")

# ============================================================
# Fix remaining Err(e) arms that weren't converted (lines 2494 and 2528)
# ============================================================
lines = content.split('\n')

# Find lines with "Err(e) =>" that follow a timeout-wrapped match
for i, line in enumerate(lines):
    stripped = line.strip()
    if stripped == 'Err(e) => {' and i > 0:
        # Check if the previous non-blank line contains the timeout pattern
        prev_line = lines[i-1].strip()
        # This is inside a timeout-wrapped match, convert to Ok(Err(e))
        indent = line[:len(line) - len(line.lstrip())]
        lines[i] = indent + 'Ok(Err(e)) => {'
        # Find the matching closing brace and add Err(_) timeout arm after it
        # Find the end of this Err arm (matching })
        depth = 0
        arm_end = i
        for j in range(i + 1, len(lines)):
            for ch in lines[j]:
                if ch == '{':
                    depth += 1
                elif ch == '}':
                    depth -= 1
                    if depth == 0:
                        arm_end = j
                        break
            if depth == 0:
                break
        # Check if arm_end is valid
        if arm_end > i:
            close_indent = lines[arm_end][:len(lines[arm_end]) - len(lines[arm_end].lstrip())]
            # Insert timeout arm before the closing brace
            lines.insert(arm_end, close_indent + 'Err(_) => {')
            lines.insert(arm_end + 1, close_indent + '    let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));')
            lines.insert(arm_end + 2, close_indent + '    let _ = tx.send(StreamEvent::Done);')
            lines.insert(arm_end + 3, close_indent + '}')
            print(f"Fixed Err -> Ok(Err) at line {i+1}, added timeout arm at line {arm_end+1}")
            changes_applied += 1

content = '\n'.join(lines)

# ============================================================
# Write the result
# ============================================================
with open('/home/synth/projects/openshark/src/tui/mod.rs', 'w') as f:
    f.write(content)

print(f"\nTotal changes applied: {changes_applied}")
print("File written successfully")
