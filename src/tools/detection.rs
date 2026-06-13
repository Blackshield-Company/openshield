/// Tool suggestion detection from model output.
///
/// Parses model responses to detect tool usage suggestions in various formats:
/// - Explicit `TOOL:tool_name args` format
/// - Markdown code blocks with `tool:tool_name` language specifier
/// - Natural language patterns like "I should use [tool] to..."
use regex::Regex;

/// A batch of tool suggestions for multi-file edit approval.
#[derive(Debug, Clone, Default)]
pub struct ToolBatch {
    pub suggestions: Vec<ToolSuggestion>,
    pub approved: Vec<bool>,
}

impl ToolBatch {
    pub fn new(suggestions: Vec<ToolSuggestion>) -> Self {
        let len = suggestions.len();
        Self {
            suggestions,
            approved: vec![false; len],
        }
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.suggestions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.suggestions.len()
    }

    pub fn approve_all(&mut self) {
        self.approved.fill(true);
    }

    pub fn reject_all(&mut self) {
        self.approved.fill(false);
    }

    #[allow(dead_code)]
    pub fn approved_suggestions(&self) -> impl Iterator<Item = &ToolSuggestion> {
        self.suggestions
            .iter()
            .zip(&self.approved)
            .filter(|(_, a)| **a)
            .map(|(s, _)| s)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSuggestion {
    /// The name of the suggested tool.
    pub tool_name: String,
    /// The arguments for the tool, if any.
    pub args: String,
    /// Confidence score (0.0 to 1.0). Higher = more certain.
    pub confidence: f32,
    /// The raw text snippet that triggered the detection.
    pub raw_text: String,
}

/// Detect tool suggestions in the given text.
///
/// Returns a vector of suggestions sorted by confidence (highest first).
/// Does not auto-execute; callers must prompt the user for approval.
pub fn detect_tool_suggestions(text: &str) -> Vec<ToolSuggestion> {
    let mut suggestions = Vec::new();

    detect_explicit_tool(text, &mut suggestions);
    detect_markdown_tool_blocks(text, &mut suggestions);
    detect_natural_language(text, &mut suggestions);

    suggestions.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

    suggestions
}

/// Parse a single tool invocation from text.
///
/// Returns the highest-confidence suggestion, if any.
#[allow(dead_code)]
pub fn parse_tool_invocation(text: &str) -> Option<ToolSuggestion> {
    let suggestions = detect_tool_suggestions(text);
    suggestions.into_iter().next()
}

fn detect_explicit_tool(text: &str, out: &mut Vec<ToolSuggestion>) {
    // Pattern: TOOL:tool_name args...  OR  TOOL: tool_name args... (with space after colon)
    //          OR  TOOL.tool_name args...  OR  TOOL. tool_name args... (with space after dot)
    // Must be at the start of a line or after whitespace.
    let re = match Regex::new(r"(?m)^\s*TOOL[:\.]\s*(\S+)(?:\s+(.*))?$") {
        Ok(r) => r,
        Err(_) => return,
    };

    for cap in re.captures_iter(text) {
        let tool_name = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        let args = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
        let raw = cap.get(0).map(|m| m.as_str()).unwrap_or("");

        if tool_name.is_empty() {
            continue;
        }

        out.push(ToolSuggestion {
            tool_name: tool_name.to_string(),
            args: args.to_string(),
            confidence: 1.0,
            raw_text: raw.to_string(),
        });
    }
}

fn detect_markdown_tool_blocks(text: &str, out: &mut Vec<ToolSuggestion>) {
    // Pattern: ```tool:tool_name
    //          args...
    //          ```
    let re = match Regex::new(r"(?s)```tool:(\S+)\n(.*?)```") {
        Ok(r) => r,
        Err(_) => return,
    };

    for cap in re.captures_iter(text) {
        let tool_name = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        let args = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
        let raw = cap.get(0).map(|m| m.as_str()).unwrap_or("");

        if tool_name.is_empty() {
            continue;
        }

        out.push(ToolSuggestion {
            tool_name: tool_name.to_string(),
            args: args.to_string(),
            confidence: 0.9,
            raw_text: raw.to_string(),
        });
    }
}

fn detect_natural_language(text: &str, out: &mut Vec<ToolSuggestion>) {
    // Patterns:
    // - "I should use [tool] to..."
    // - "Let me [tool]..."
    // - "I'll [tool]..."
    // - "I will [tool]..."
    // - "Using [tool]..."
    // - "I can [tool]..."
    //
    // We look for known tool names after these phrases.
    // To keep it conservative, we match on a bounded list of tool names.
    let tool_names = [
        "edit",
        "fs",
        "git",
        "lsp",
        "refactor",
        "search",
        "grep",
        "terminal",
        "test_runner",
        "web",
        "browser",
        "x_search",
        "vision",
        "image_gen",
        "video",
        "video_gen",
        "tts",
        "memory",
        "session_search",
        "context_engine",
        "todo",
        "cronjob",
        "skills",
        "messaging",
        "homeassistant",
        "spotify",
        "yuanbao",
        "computer_use",
        "moa",
        "delegation",
        "clarify",
        "code_execution",
    ];

    let patterns = [
        // "I should use search to find..."
        Regex::new(r"(?i)I\s+should\s+use\s+(\w+)\s+to\b").ok(),
        // "Let me search for..."
        Regex::new(r"(?i)Let\s+me\s+(\w+)\b").ok(),
        // "I'll search for..."
        Regex::new(r"(?i)I'll\s+(\w+)\b").ok(),
        // "I will search for..."
        Regex::new(r"(?i)I\s+will\s+(\w+)\b").ok(),
        // "Using search..."
        Regex::new(r"(?i)Using\s+(\w+)\b").ok(),
        // "I can search for..."
        Regex::new(r"(?i)I\s+can\s+(\w+)\b").ok(),
    ];

    for pat in patterns.iter().flatten() {
        for cap in pat.captures_iter(text) {
            let tool_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let raw = cap.get(0).map(|m| m.as_str()).unwrap_or("");

            if tool_name.is_empty() {
                continue;
            }

            // Only accept if the captured word is a known tool name.
            if !tool_names.contains(&tool_name.to_ascii_lowercase().as_str()) {
                continue;
            }

            // Extract args: everything after the matched phrase up to end of sentence or line.
            let args = extract_args_after(text, raw);

            out.push(ToolSuggestion {
                tool_name: tool_name.to_ascii_lowercase(),
                args,
                confidence: 0.6,
                raw_text: raw.to_string(),
            });
        }
    }
}

/// Extract arguments from the text after the matched phrase.
/// Stops at sentence boundaries (. ! ?) or newlines.
fn extract_args_after(text: &str, matched: &str) -> String {
    if let Some(pos) = text.find(matched) {
        let after = &text[pos + matched.len()..];
        let trimmed = after.trim_start();
        // Stop at sentence end or newline
        let end = trimmed.find(['.', '!', '?', '\n']).unwrap_or(trimmed.len());
        trimmed[..end].trim().to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explicit_tool_format() {
        let text = "TOOL:search find all TODOs";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "search");
        assert_eq!(suggestions[0].args, "find all TODOs");
        assert!((suggestions[0].confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_explicit_tool_with_space_after_colon() {
        // Model sometimes outputs "TOOL: fs cat" instead of "TOOL:fs cat"
        let text = "TOOL: fs cat /home/synth/projects/README.md";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "fs");
        assert_eq!(suggestions[0].args, "cat /home/synth/projects/README.md");
        assert!((suggestions[0].confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_explicit_tool_no_args() {
        let text = "TOOL:git";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "git");
        assert_eq!(suggestions[0].args, "");
        assert!((suggestions[0].confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_explicit_tool_multiline() {
        let text = "Some text before\nTOOL:edit src/main.rs\nMore text after";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "edit");
        assert_eq!(suggestions[0].args, "src/main.rs");
    }

    #[test]
    fn test_explicit_tool_with_dot_separator() {
        // Model sometimes outputs "TOOL.fs cat" instead of "TOOL:fs cat"
        let text = "TOOL.fs cat /home/synth/projects/README.md";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "fs");
        assert_eq!(suggestions[0].args, "cat /home/synth/projects/README.md");
        assert!((suggestions[0].confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_explicit_tool_with_space_after_dot() {
        let text = "TOOL. fs cat /home/synth/projects/README.md";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "fs");
        assert_eq!(suggestions[0].args, "cat /home/synth/projects/README.md");
    }

    #[test]
    fn test_multiple_embedded_tools() {
        let text = "I see the grid already has some vectors running.\nTOOL: fs cat /home/synth/projects/README.md\nTOOL: fs tree /home/synth/projects/0x7-web 2\nLet me analyze...";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].tool_name, "fs");
        assert_eq!(suggestions[0].args, "cat /home/synth/projects/README.md");
        assert_eq!(suggestions[1].tool_name, "fs");
        assert_eq!(suggestions[1].args, "tree /home/synth/projects/0x7-web 2");
    }

    #[test]
    fn test_markdown_tool_block() {
        let text = r#"I think we should do this:
```tool:search
find all TODOs
```
Let me know if that helps."#;
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "search");
        assert_eq!(suggestions[0].args, "find all TODOs");
        assert!((suggestions[0].confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_markdown_tool_block_no_args() {
        let text = r#"```tool:git
```"#;
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "git");
        assert_eq!(suggestions[0].args, "");
    }

    #[test]
    fn test_natural_language_should_use() {
        let text = "I should use search to find all TODOs in the codebase.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "search");
        assert_eq!(suggestions[0].args, "find all TODOs in the codebase");
        assert!((suggestions[0].confidence - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn test_natural_language_let_me() {
        let text = "Let me search for all TODOs.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "search");
        assert_eq!(suggestions[0].args, "for all TODOs");
    }

    #[test]
    fn test_natural_language_ill() {
        let text = "I'll grep for the function name.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "grep");
        assert_eq!(suggestions[0].args, "for the function name");
    }

    #[test]
    fn test_natural_language_i_will() {
        let text = "I will refactor the code to use better names.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "refactor");
        assert_eq!(suggestions[0].args, "the code to use better names");
    }

    #[test]
    fn test_natural_language_using() {
        let text = "Using terminal to run the tests.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "terminal");
        assert_eq!(suggestions[0].args, "to run the tests");
    }

    #[test]
    fn test_natural_language_i_can() {
        let text = "I can edit src/main.rs to fix the bug.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "edit");
        assert_eq!(suggestions[0].args, "src/main");
    }

    #[test]
    fn test_natural_language_unknown_tool_ignored() {
        let text = "I should use foobar to do something.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 0);
    }

    #[test]
    fn test_multiple_suggestions_deduplicated() {
        let text = "TOOL:search find TODOs\nI should use search to find FIXMEs.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].tool_name, "search");
        assert_eq!(suggestions[0].args, "find TODOs");
        assert!((suggestions[0].confidence - 1.0).abs() < f32::EPSILON);
        assert_eq!(suggestions[1].tool_name, "search");
        assert_eq!(suggestions[1].args, "find FIXMEs");
    }

    #[test]
    fn test_no_suggestions() {
        let text = "This is just regular text with no tool suggestions.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 0);
    }

    #[test]
    fn test_parse_tool_invocation_returns_best() {
        let text = "I should use search to find TODOs.\nTOOL:grep pattern";
        let suggestion = parse_tool_invocation(text);
        assert!(suggestion.is_some());
        let s = suggestion.unwrap();
        assert_eq!(s.tool_name, "grep");
        assert_eq!(s.args, "pattern");
        assert!((s.confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_tool_invocation_none() {
        let text = "Just chatting here.";
        let suggestion = parse_tool_invocation(text);
        assert!(suggestion.is_none());
    }

    #[test]
    fn test_case_insensitive_natural_language() {
        let text = "I SHOULD USE SEARCH to find things.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "search");
    }

    #[test]
    fn test_tool_name_case_normalized() {
        let text = "Let me Search for things.";
        let suggestions = detect_tool_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tool_name, "search");
    }
}
