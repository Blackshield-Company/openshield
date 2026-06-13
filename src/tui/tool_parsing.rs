#![allow(dead_code)]

pub(crate) fn extract_thinking_from_chunk(chunk: &str) -> (Option<String>, String) {
    let mut reasoning = String::new();
    let mut content = String::new();
    let mut remaining = chunk;

    while !remaining.is_empty() {
        if let Some(start) = remaining.find("<think>") {
            // Add text before <think> to content
            content.push_str(&remaining[..start]);
            let after_start = &remaining[start + 7..];
            if let Some(end) = after_start.find("</think>") {
                // Complete think block
                reasoning.push_str(&after_start[..end]);
                remaining = &after_start[end + 8..];
            } else {
                // Incomplete think block — rest is reasoning
                reasoning.push_str(after_start);
                remaining = "";
            }
        } else {
            // No more think tags
            content.push_str(remaining);
            remaining = "";
        }
    }

    let reasoning_opt = if reasoning.is_empty() {
        None
    } else {
        Some(reasoning)
    };
    (reasoning_opt, content)
}
pub(crate) fn parse_embedded_tools(text: &str) -> Vec<(String, String)> {
    let mut tools = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        // Handle both "TOOL:tool_name args" and "TOOL.tool_name args"
        let prefix = if trimmed.starts_with("TOOL:") {
            Some("TOOL:")
        } else if trimmed.starts_with("TOOL.") {
            Some("TOOL.")
        } else {
            None
        };
        if let Some(p) = prefix {
            let rest = &trimmed[p.len()..]; // after "TOOL:" or "TOOL."
            let rest = rest.trim_start(); // handle "TOOL: fs cat" → "fs cat"
            // Try JSON format first: tool_name:0>{"args":"query"}
            if let Some((name, args)) = parse_json_tool_format(rest) {
                tools.push((name, args));
            } else {
                // Original space-split format: tool_name args
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                if !parts.is_empty() && !parts[0].is_empty() {
                    let tool_name = parts[0].trim().to_string();
                    let args = parts.get(1).unwrap_or(&"").trim().to_string();
                    tools.push((tool_name, args));
                }
            }
        }
    }
    tools
}
pub(crate) fn parse_json_tool_format(rest: &str) -> Option<(String, String)> {
    // Try bare JSON format first: tool_name {"key": "value"}
    if let Some(space_pos) = rest.find(['{', ':'])
        && rest.as_bytes().get(space_pos) == Some(&b'{')
    {
        let tool_name = rest[..space_pos].trim().to_string();
        if tool_name.is_empty() {
            return None;
        }
        let json_str = find_balanced_json(&rest[space_pos..])?;
        return extract_args_from_json(json_str, &tool_name);
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
pub(crate) fn find_balanced_json(s: &str) -> Option<&str> {
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
        if ch == '\\' {
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
pub(crate) fn extract_args_from_json(json_str: &str, tool_name: &str) -> Option<(String, String)> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(args) = v.get("args").and_then(|a| a.as_str()) {
            return Some((tool_name.to_string(), args.to_string()));
        }
        if let Some(args) = v.get("query").and_then(|a| a.as_str()) {
            return Some((tool_name.to_string(), args.to_string()));
        }

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

        if tool_name == "edit" && !parts.is_empty() {
            let file = parts.first().cloned().unwrap_or_default();
            if file.is_empty() {
                return None;
            }
            let old_str = parts.get(1).cloned().unwrap_or_default();
            let new_str = parts.get(2).cloned().unwrap_or_default();
            if !old_str.is_empty() {
                return Some((
                    tool_name.to_string(),
                    format!("replace {}\n{}\n---\n{}", file, old_str, new_str),
                ));
            }
            return Some((tool_name.to_string(), format!("read {}", file)));
        }
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
pub(crate) fn extract_generic_args(
    v: serde_json::Value,
    tool_name: &str,
) -> Option<(String, String)> {
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
}
pub(crate) fn strip_tool_lines(text: &str) -> String {
    let mut result = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("TOOL:") && !trimmed.starts_with("TOOL.") {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
        }
    }
    result
}
pub(crate) fn extract_thinking(text: &str) -> String {
    let start = text.find("<think>");
    let end = text.find("</think>");
    match (start, end) {
        (Some(s), Some(e)) if e > s => text[s + 7..e].trim().to_string(),
        _ => String::new(),
    }
}
pub(crate) fn strip_think_tags(text: &str) -> String {
    let mut result = text.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result.find("</think>") {
            result.replace_range(start..end + 8, "");
        } else {
            break;
        }
    }
    result.trim().to_string()
}
pub(crate) fn generate_edit_diff(args: &str) -> Option<String> {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return None;
    }

    let cmd = parts[0];
    let rest = parts[1];

    match cmd {
        "write" => {
            let wparts: Vec<&str> = rest.splitn(2, ' ').collect();
            if wparts.len() < 2 {
                return None;
            }
            let path = wparts[0];
            let content = wparts[1];
            crate::diff::preview_write(path, content).ok()
        }
        "replace" => {
            let delimiter = " ||| ";
            let rp: Vec<&str> = rest.splitn(2, delimiter).collect();
            if rp.len() < 2 {
                return None;
            }
            let pp: Vec<&str> = rp[0].splitn(2, ' ').collect();
            if pp.len() < 2 {
                return None;
            }
            let path = pp[0];
            let old_str = pp[1];
            let new_str = rp[1];
            crate::diff::preview_replace(path, old_str, new_str).ok()
        }
        "patch" => {
            let delimiter = " ||| ";
            let pp: Vec<&str> = rest.splitn(2, delimiter).collect();
            if pp.len() < 2 {
                return None;
            }
            let ppp: Vec<&str> = pp[0].splitn(2, ' ').collect();
            if ppp.len() < 2 {
                return None;
            }
            let path = ppp[0];
            let old_lines = ppp[1];
            let new_lines = pp[1];
            crate::diff::preview_patch(path, old_lines, new_lines).ok()
        }
        _ => None,
    }
}

/// Check if a tool name is an edit operation that should trigger auto-commit.
pub(crate) fn is_edit_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "edit" | "write" | "patch" | "replace" | "refactor"
    )
}
