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
            "edit" => &["operation", "file", "old_string", "new_string"],
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
            let has_operation = parts.len() >= 4;
            let operation = if has_operation {
                parts[0].clone()
            } else {
                String::new()
            };
            let file_idx = if has_operation { 1 } else { 0 };
            let old_idx = if has_operation { 2 } else { 1 };
            let new_idx = if has_operation { 3 } else { 2 };

            let file = parts.get(file_idx).cloned().unwrap_or_default();
            if file.is_empty() {
                return None;
            }
            let old_str = parts.get(old_idx).cloned().unwrap_or_default();
            let new_str = parts.get(new_idx).cloned().unwrap_or_default();

            let op = if !operation.is_empty() {
                operation
            } else if old_str.is_empty() && !new_str.is_empty() {
                "write".to_string()
            } else if !old_str.is_empty() {
                "replace".to_string()
            } else {
                "read".to_string()
            };

            match op.as_str() {
                "write" => {
                    return Some((tool_name.to_string(), format!("write {} {}", file, new_str)));
                }
                "replace" => {
                    return Some((
                        tool_name.to_string(),
                        format!("replace {} {} ||| {}", file, old_str, new_str),
                    ));
                }
                "patch" => {
                    return Some((
                        tool_name.to_string(),
                        format!("patch {} {} ||| {}", file, old_str, new_str),
                    ));
                }
                _ => return Some((tool_name.to_string(), format!("read {}", file))),
            }
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

/// Inline scrollback render for a code-writing tool call (edit/write/patch).
/// The body is plain text (one diff/preview line per row, already capped);
/// coloring is applied by the renderer so it can wrap lines first.
#[derive(Debug, Clone)]
pub(crate) struct InlineToolRender {
    /// "write" | "replace" | "patch"
    pub op: &'static str,
    pub path: String,
    pub body: String,
}

/// Cap for edit/patch inline diffs (body lines) before a truncation footer.
const INLINE_DIFF_MAX_LINES: usize = 60;
/// Cap for whole-file write previews (body lines) before a truncation footer.
const WRITE_PREVIEW_MAX_LINES: usize = 30;

/// Build an inline diff/preview for a code-writing tool call, parsed from the
/// tool-call arguments (JSON or normalized mini-language) — never re-reads
/// files from disk. Returns None for non-edit tools or unparseable args.
pub(crate) fn build_inline_tool_render(tool_name: &str, args: &str) -> Option<InlineToolRender> {
    let (op, path, old, new) = parse_edit_args(tool_name, args)?;

    let body = match op {
        "write" => {
            // Whole-file write: preview the content being written as +lines.
            let lines: Vec<String> = new.lines().map(|l| format!("+{l}")).collect();
            cap_body(lines, WRITE_PREVIEW_MAX_LINES)
        }
        _ => {
            // replace/patch: inline diff of old vs new content from the args.
            let lines = inline_diff_lines(&old, &new);
            cap_body(lines, INLINE_DIFF_MAX_LINES)
        }
    };

    Some(InlineToolRender {
        op: match op {
            "write" => "write",
            "patch" => "patch",
            _ => "replace",
        },
        path,
        body,
    })
}

/// Extract (operation, path, old_content, new_content) from edit-tool args.
/// Accepts both raw JSON arguments and the normalized mini-language
/// ("write <path> <content>", "replace <path> <old> ||| <new>").
fn parse_edit_args<'a>(
    tool_name: &str,
    args: &'a str,
) -> Option<(&'static str, String, String, String)> {
    let trimmed = args.trim();
    if trimmed.starts_with('{') {
        parse_edit_args_json(tool_name, trimmed)
    } else {
        parse_edit_args_mini(tool_name, trimmed)
    }
}

fn parse_edit_args_json(
    tool_name: &str,
    json_str: &str,
) -> Option<(&'static str, String, String, String)> {
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let get_str = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
            .map(|s| s.to_string())
    };

    let path = get_str(&["file", "path", "filename"])?;
    let old = get_str(&["old_string", "old_lines", "old", "old_content"]).unwrap_or_default();
    let new =
        get_str(&["new_string", "new_lines", "new_content", "content", "new"]).unwrap_or_default();

    let op = get_str(&["operation"])
        .map(|s| s.to_lowercase())
        .filter(|s| matches!(s.as_str(), "write" | "replace" | "patch"))
        .unwrap_or_else(|| {
            if !old.is_empty() || new.is_empty() {
                "replace".to_string()
            } else {
                "write".to_string()
            }
        });

    if new.is_empty() && old.is_empty() {
        return None;
    }

    let _ = tool_name;
    let op: &'static str = match op.as_str() {
        "write" => "write",
        "patch" => "patch",
        _ => "replace",
    };
    Some((op, path, old, new))
}

fn parse_edit_args_mini<'a>(
    tool_name: &str,
    args: &'a str,
) -> Option<(&'static str, String, String, String)> {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return None;
    }
    let cmd = parts[0];
    let rest = parts[1];

    // Only the edit family carries old/new content in the mini-language.
    if !matches!(cmd, "write" | "replace" | "patch") {
        let _ = tool_name;
        return None;
    }

    match cmd {
        "write" => {
            let wp: Vec<&str> = rest.splitn(2, ' ').collect();
            if wp.len() < 2 || wp[0].is_empty() {
                return None;
            }
            Some(("write", wp[0].to_string(), String::new(), wp[1].to_string()))
        }
        "replace" | "patch" => {
            let delimiter = " ||| ";
            let rp: Vec<&str> = rest.splitn(2, delimiter).collect();
            if rp.len() < 2 {
                return None;
            }
            let pp: Vec<&str> = rp[0].splitn(2, ' ').collect();
            if pp.len() < 2 || pp[0].is_empty() {
                return None;
            }
            let op: &'static str = if cmd == "patch" { "patch" } else { "replace" };
            Some((op, pp[0].to_string(), pp[1].to_string(), rp[1].to_string()))
        }
        _ => None,
    }
}

/// Diff old vs new line-by-line using a common prefix/suffix split: context
/// lines (2 before/after the change) marked " ", removals "-", additions "+".
fn inline_diff_lines(old: &str, new: &str) -> Vec<String> {
    let old_l: Vec<&str> = old.lines().collect();
    let new_l: Vec<&str> = new.lines().collect();

    let mut start = 0;
    while start < old_l.len() && start < new_l.len() && old_l[start] == new_l[start] {
        start += 1;
    }
    let mut old_end = old_l.len();
    let mut new_end = new_l.len();
    while old_end > start && new_end > start && old_l[old_end - 1] == new_l[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }

    let mut out = Vec::new();
    let ctx_start = start.saturating_sub(2);
    for line in &old_l[ctx_start..start] {
        out.push(format!(" {line}"));
    }
    for line in &old_l[start..old_end] {
        out.push(format!("-{line}"));
    }
    for line in &new_l[start..new_end] {
        out.push(format!("+{line}"));
    }
    let ctx_end = (old_end + 2).min(old_l.len());
    for line in &old_l[old_end..ctx_end] {
        out.push(format!(" {line}"));
    }
    out
}

/// Join body lines, appending a "... N more lines" footer when capped.
fn cap_body(mut lines: Vec<String>, max: usize) -> String {
    if lines.len() > max {
        let hidden = lines.len() - max;
        lines.truncate(max);
        lines.push(format!("… {hidden} more lines"));
    }
    lines.join("\n")
}

/// Check if a tool name is an edit operation that should trigger auto-commit.
pub(crate) fn is_edit_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "edit" | "write" | "patch" | "replace" | "refactor"
    )
}

#[cfg(test)]
mod inline_render_tests {
    use super::*;

    #[test]
    fn json_replace_produces_colored_diff_body() {
        let args = r#"{"operation":"replace","file":"src/main.rs","old_string":"let x = 1;\nlet y = 2;","new_string":"let x = 1;\nlet y = 3;"}"#;
        let render = build_inline_tool_render("edit", args).unwrap();
        assert_eq!(render.op, "replace");
        assert_eq!(render.path, "src/main.rs");
        assert!(render.body.contains("-let y = 2;"));
        assert!(render.body.contains("+let y = 3;"));
        // Common line appears as context.
        assert!(render.body.contains(" let x = 1;"));
    }

    #[test]
    fn json_write_produces_preview_with_plus_lines() {
        let args = r#"{"operation":"write","file":"/tmp/new.rs","new_string":"fn main() {\n    println!(\"hi\");\n}"}"#;
        let render = build_inline_tool_render("edit", args).unwrap();
        assert_eq!(render.op, "write");
        assert!(render.body.contains("+fn main() {"));
        assert!(!render.body.contains('-'));
    }

    #[test]
    fn json_operation_inferred_when_missing() {
        // No "operation": old_string present -> replace.
        let args = r#"{"file":"a.rs","old_string":"foo","new_string":"bar"}"#;
        let render = build_inline_tool_render("edit", args).unwrap();
        assert_eq!(render.op, "replace");
        // old empty + content present -> write.
        let args = r#"{"file":"a.rs","content":"hello"}"#;
        let render = build_inline_tool_render("edit", args).unwrap();
        assert_eq!(render.op, "write");
    }

    #[test]
    fn mini_language_args_are_supported() {
        let args = "replace src/lib.rs old_fn() {} ||| new_fn() {}";
        let render = build_inline_tool_render("edit", args).unwrap();
        assert_eq!(render.path, "src/lib.rs");
        assert!(render.body.contains("-old_fn() {}"));
        assert!(render.body.contains("+new_fn() {}"));
    }

    #[test]
    fn non_edit_tools_return_none() {
        assert!(build_inline_tool_render("search", r#"{"query":"x"}"#).is_none());
        assert!(build_inline_tool_render("edit", "").is_none());
    }

    #[test]
    fn large_write_is_capped_with_footer() {
        let content: Vec<String> = (0..100).map(|i| format!("line{i}")).collect();
        let args = format!(
            r#"{{"operation":"write","file":"big.rs","new_string":{}}}"#,
            serde_json::to_string(&content.join("\n")).unwrap()
        );
        let render = build_inline_tool_render("edit", &args).unwrap();
        let body_lines: Vec<&str> = render.body.lines().collect();
        assert_eq!(body_lines.len(), WRITE_PREVIEW_MAX_LINES + 1);
        assert!(body_lines.last().unwrap().contains("more lines"));
    }

    #[test]
    fn large_diff_is_capped_with_footer() {
        let old: Vec<String> = (0..50).map(|i| format!("old{i}")).collect();
        let new: Vec<String> = (0..50).map(|i| format!("new{i}")).collect();
        let args = format!("replace big.rs {} ||| {}", old.join("\n"), new.join("\n"));
        let render = build_inline_tool_render("edit", &args).unwrap();
        let body_lines: Vec<&str> = render.body.lines().collect();
        assert_eq!(body_lines.len(), INLINE_DIFF_MAX_LINES + 1);
        assert!(body_lines.last().unwrap().contains("more lines"));
    }
}
