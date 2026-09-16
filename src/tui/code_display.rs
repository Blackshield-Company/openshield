//! Visible code-writing display for the unified chat feed.
//!
//! Detects fenced code blocks in assistant content and renders them as a
//! bordered, syntax-highlighted panel. Streaming content is re-rendered from
//! the accumulated buffer on every chunk, so an unclosed fence shows a live
//! "writing…" footer that moves down as code arrives.

use crossterm::style::Color;
use unicode_width::UnicodeWidthChar;

use super::syntax_highlight::highlight_code_block;
use super::theme::{ansi_fg, ansi_reset, current_theme};

/// Render message/stream content into display lines. Normal prose is wrapped
/// and tinted `base`; fenced code blocks become bordered + highlighted rows.
pub(crate) fn render_content_lines(content: &str, width: usize, base: Color) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_code = false;
    let mut lang = String::new();
    let mut code: Vec<String> = Vec::new();

    for raw in content.lines() {
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_code {
                out.extend(render_code_block(&code, &lang, width));
                out.push(border_line("└─", width, false));
                code.clear();
                lang.clear();
                in_code = false;
            } else {
                in_code = true;
                lang = rest.trim().to_string();
                out.push(border_line(
                    &format!("┌─ code{}", lang_suffix(&lang)),
                    width,
                    false,
                ));
            }
            continue;
        }

        if in_code {
            code.push(raw.to_string());
        } else {
            for w in wrap_line(raw, width) {
                out.push(format!("{}{}{}", ansi_fg(base), w, ansi_reset()));
            }
        }
    }

    // Partial fence while streaming: still show the code panel, with a live
    // footer instead of leaving the block visually open-ended.
    if in_code {
        out.extend(render_code_block(&code, &lang, width));
        out.push(border_line("└─ writing…", width, true));
    }

    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn lang_suffix(lang: &str) -> String {
    if lang.is_empty() {
        String::new()
    } else {
        format!(": {lang}")
    }
}

fn border_line(label: &str, width: usize, pulse: bool) -> String {
    let theme = current_theme();
    let color = if pulse {
        theme.highlight
    } else {
        theme.accent_tertiary
    };
    // Keep borders short and deterministic; the code body carries the signal.
    let body = if width > 24 {
        format!(
            "{label} {}",
            "─".repeat(width.saturating_sub(label.chars().count() + 2).min(18))
        )
    } else {
        label.to_string()
    };
    format!("{}{}{}", ansi_fg(color), body, ansi_reset())
}

fn render_code_block(code: &[String], lang: &str, width: usize) -> Vec<String> {
    let code_width = width.saturating_sub(2).max(1);
    let mut out = Vec::new();
    for line in code {
        if line.is_empty() {
            out.push(format!(
                "{}  {}",
                ansi_fg(current_theme().muted),
                ansi_reset()
            ));
            continue;
        }
        for segment in wrap_line(line, code_width) {
            // Wrap before highlighting so ANSI escapes never distort width math.
            // Per-segment highlighting is approximate across wrapped tokens, but
            // stable for the feed and exact for mouse-selection row mapping.
            let highlighted = highlight_code_block(&segment, lang)
                .into_iter()
                .next()
                .unwrap_or(segment);
            out.push(format!("  {highlighted}"));
        }
    }
    if code.is_empty() {
        out.push(format!(
            "{}  …{}",
            ansi_fg(current_theme().muted),
            ansi_reset()
        ));
    }
    out
}

/// Simple word-wrap that respects display width (mirrors components::chat).
fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_string()];
    }
    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in line.chars() {
        let ch_width = ch.width().unwrap_or(1);
        if current_width + ch_width > width && !current.is_empty() {
            result.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() {
        result.push(current);
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[String]) -> String {
        let mut out = String::new();
        let joined = lines.join("\n");
        let mut chars = joined.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                // Skip CSI sequence: ESC '[' parameters ... final byte ('m').
                if chars.next() == Some('[') {
                    for c in chars.by_ref() {
                        if c == 'm' {
                            break;
                        }
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn fenced_code_gets_borders_and_highlighting() {
        let lines = render_content_lines(
            "before\n```rust\nfn main() {}\n```\nafter",
            80,
            Color::White,
        );
        let joined = plain(&lines);
        assert!(joined.contains("┌─ code: rust"));
        assert!(joined.contains("fn main() {}"));
        assert!(joined.contains("└─"));
        assert!(joined.contains("before"));
        assert!(joined.contains("after"));
    }

    #[test]
    fn unclosed_streaming_fence_shows_live_footer() {
        let lines = render_content_lines("```python\nprint('hi')", 80, Color::White);
        let joined = plain(&lines);
        assert!(joined.contains("┌─ code: python"));
        assert!(joined.contains("print('hi')"));
        assert!(joined.contains("└─ writing…"));
    }

    #[test]
    fn prose_still_wraps() {
        let lines = render_content_lines(&"word ".repeat(30), 10, Color::White);
        assert!(lines.len() > 1);
    }
}
