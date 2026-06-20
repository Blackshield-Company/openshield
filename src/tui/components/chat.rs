use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    style::{Print, ResetColor},
    queue,
};

use crate::tui::theme::*;
use crate::tui::{App, ChatMessage};

/// Draw the unified message feed — full width, no borders, everything scrolls.
/// `area` is (x, y, width, height) in terminal coordinates.
///
/// ANTI-FLICKER: no per-line Clear(UntilNewLine), no flush. The caller handles
/// clearing the entire screen once and flushing once at frame end.
pub fn draw_unified_feed(
    out: &mut impl Write,
    app: &App,
    area: (u16, u16, u16, u16),
) -> io::Result<()> {
    let (x, y, width, height) = area;

    let inner_width = width.saturating_sub(2); // 1-char padding each side
    let inner_height = height as usize;

    // Build a flat list of all visible lines in the feed:
    // system info header + messages + streaming content + reasoning
    let mut all_lines: Vec<String> = Vec::new();

    // ── System info header (scrolls with feed) ───────────────────────────────
    all_lines.push(format_info_line("Model", &app.model, inner_width as usize));
    all_lines.push(format_info_line(
        "Session",
        &app.session_id[..app.session_id.len().min(24)],
        inner_width as usize,
    ));
    all_lines.push(format_info_line(
        "Ctx",
        &format!("{} / {}", app.context_used(), app.model_context_length),
        inner_width as usize,
    ));
    all_lines.push(format_info_line(
        "Tokens",
        &app.tokens_used.to_string(),
        inner_width as usize,
    ));
    all_lines.push(format_info_line(
        "Tools",
        &app.tool_calls_count.to_string(),
        inner_width as usize,
    ));
    all_lines.push(format_info_line("Branch", "main", inner_width as usize));

    // Separator
    all_lines.push(format!(
        "{}─{}",
        ansi_fg(Color::Rgb {
            r: 140,
            g: 120,
            b: 160,
        }),
        ansi_reset()
    ));

    // ── Messages ────────────────────────────────────────────────────────────
    for (idx, msg) in app.messages.iter().enumerate() {
        let is_selected =
            app.mode == crate::tui::AppMode::CopySelect && app.copy_selected_idx == Some(idx);
        let msg_lines = format_message(msg, inner_width as usize, is_selected);
        all_lines.extend(msg_lines);
        // Thin separator between messages
        all_lines.push(String::new());
    }

    // ── Streaming content (if active) ───────────────────────────────────────
    if app.is_streaming && !app.streaming_content.is_empty() {
        let streaming_lines =
            format_streaming_content(&app.streaming_content, inner_width as usize);
        all_lines.extend(streaming_lines);
    }

    // ── Reasoning content (if active) ───────────────────────────────────────
    if app.is_reasoning && !app.reasoning_content.is_empty() {
        let reasoning_lines =
            format_reasoning_content(&app.reasoning_content, inner_width as usize);
        all_lines.extend(reasoning_lines);
    }

    // Calculate scroll offset
    let total_lines = all_lines.len();
    let scroll = app.scroll.min(total_lines.saturating_sub(inner_height));

    // Draw visible lines — NO Clear(UntilNewLine), just Print. The screen was
    // already cleared once by the caller at frame start.
    let mut current_row = 0u16;
    for line in all_lines.iter().skip(scroll).take(inner_height) {
        let draw_x = x + 1; // 1-char left padding
        queue!(
            out,
            MoveTo(draw_x, y + current_row),
            Print(line),
            ResetColor,
        )?;
        current_row += 1;
    }

    // Clear remaining rows — only needed for the bottom of the feed area when
    // there aren't enough lines to fill it. Still no per-line clear needed
    // because the screen was cleared at frame start.
    for row in current_row..height {
        queue!(
            out,
            MoveTo(x + 1, y + row),
            Print(" "),
            ResetColor,
        )?;
    }

    Ok(())
}

/// Format a single chat message into displayable strings with ANSI colors.
/// When `is_selected` is true, the message header is highlighted to indicate
/// it will be copied on Enter.
fn format_message(msg: &ChatMessage, width: usize, is_selected: bool) -> Vec<String> {
    let mut lines = Vec::new();

    let (role_icon, role_color, role_name) = match msg.role.as_str() {
        "user" => (
            "👤",
            Color::Rgb {
                r: 255,
                g: 215,
                b: 0,
            },
            "You",
        ), // Gold
        "assistant" => (
            "🦈",
            Color::Rgb {
                r: 255,
                g: 77,
                b: 158,
            },
            "Shark",
        ), // Pink
        "system" => (
            "📋",
            Color::Rgb {
                r: 140,
                g: 120,
                b: 160,
            },
            "System",
        ), // Muted
        _ => (
            "❓",
            Color::Rgb {
                r: 220,
                g: 220,
                b: 220,
            },
            "Unknown",
        ),
    };

    // Role header with icon and name
    let header = if is_selected {
        format!(
            "{}▶ {} {} {}{}{}",
            ansi_fg(Color::Rgb {
                r: 255,
                g: 215,
                b: 0,
            }),
            role_icon,
            role_name,
            ansi_fg(Color::Rgb {
                r: 140,
                g: 120,
                b: 160,
            }),
            " [COPY]",
            ansi_reset()
        )
    } else {
        format!(
            "{}{} {} {}{}",
            ansi_fg(role_color),
            role_icon,
            role_name,
            ansi_fg(Color::Rgb {
                r: 140,
                g: 120,
                b: 160,
            }),
            ansi_reset()
        )
    };
    lines.push(header);

    // Content lines (word-wrapped)
    for content_line in msg.content.lines() {
        let wrapped = wrap_line(content_line, width.saturating_sub(2));
        for w in wrapped {
            lines.push(format!(
                "{}{}{}",
                ansi_fg(Color::Rgb {
                    r: 220,
                    g: 220,
                    b: 220,
                }),
                w,
                ansi_reset()
            ));
        }
    }

    // Multi-model responses
    for response in &msg.multi_model_responses {
        lines.push(format!(
            "{}  ↳ {} ({}ms, {}tok){}",
            ansi_fg(Color::Rgb {
                r: 140,
                g: 120,
                b: 160,
            }),
            response.model_name,
            response.latency_ms,
            response.tokens,
            ansi_reset()
        ));
        for line in response.content.lines() {
            let wrapped = wrap_line(line, width.saturating_sub(6));
            for w in wrapped {
                lines.push(format!(
                    "{}    {}{}",
                    ansi_fg(Color::Rgb {
                        r: 140,
                        g: 120,
                        b: 160,
                    }),
                    w,
                    ansi_reset()
                ));
            }
        }
    }

    lines
}

fn format_streaming_content(content: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let header = format!(
        "{}🦈 Shark {}(streaming…){}",
        ansi_fg(Color::Rgb {
            r: 255,
            g: 77,
            b: 158,
        }),
        ansi_fg(Color::Rgb {
            r: 140,
            g: 120,
            b: 160,
        }),
        ansi_reset()
    );
    lines.push(header);

    for line in content.lines() {
        let wrapped = wrap_line(line, width.saturating_sub(2));
        for w in wrapped {
            lines.push(format!(
                "{}{}{}",
                ansi_fg(Color::Rgb {
                    r: 220,
                    g: 220,
                    b: 220,
                }),
                w,
                ansi_reset()
            ));
        }
    }
    lines
}

fn format_reasoning_content(content: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let header = format!(
        "{}💭 Reasoning {}(thinking…){}",
        ansi_fg(Color::Rgb {
            r: 140,
            g: 120,
            b: 160,
        }),
        ansi_fg(Color::Rgb {
            r: 100,
            g: 100,
            b: 120,
        }),
        ansi_reset()
    );
    lines.push(header);

    for line in content.lines() {
        let wrapped = wrap_line(line, width.saturating_sub(2));
        for w in wrapped {
            lines.push(format!(
                "{}{}{}",
                ansi_fg(Color::Rgb {
                    r: 140,
                    g: 140,
                    b: 160,
                }),
                w,
                ansi_reset()
            ));
        }
    }
    lines
}

fn format_info_line(label: &str, value: &str, width: usize) -> String {
    let label_color = ansi_fg(Color::Rgb {
        r: 140,
        g: 120,
        b: 160,
    });
    let value_color = ansi_fg(Color::Rgb {
        r: 220,
        g: 220,
        b: 220,
    });
    let reset = ansi_reset();
    let label_w = 10usize;
    let value_w = width.saturating_sub(label_w + 2);
    format!(
        "{}{:>label_w$}{} {}{:<value_w$}{}",
        label_color,
        label,
        reset,
        value_color,
        value.chars().take(value_w).collect::<String>(),
        reset,
        label_w = label_w,
        value_w = value_w
    )
}

/// Simple word-wrap that respects display width.
fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_string()];
    }
    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in line.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
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

/// Calculate the number of display lines needed for the input text.
/// Handles newlines and Unicode width properly.
pub(crate) fn calculate_input_lines(text: &str, available_width: usize) -> usize {
    if text.is_empty() || available_width == 0 {
        return 1;
    }

    let mut total_lines = 0;
    for line in text.split('\n') {
        let wrapped = wrap_line(line, available_width);
        total_lines += wrapped.len().max(1);
    }

    total_lines.max(1)
}

/// Wrap input text into display lines, respecting newlines and Unicode width.
pub(crate) fn wrap_input_text(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() || max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    for line in text.split('\n') {
        let wrapped = wrap_line(line, max_width);
        for w in wrapped {
            lines.push(w);
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Draw the input bar at the bottom — clean, no borders, full width.
/// Just a `>` prompt with the input text.
///
/// ANTI-FLICKER: no flush. Caller handles it at frame end.
pub fn draw_input_bar(
    out: &mut impl Write,
    app: &App,
    area: (u16, u16, u16, u16),
) -> io::Result<()> {
    let (x, y, width, height) = area;

    let gold = Color::Rgb {
        r: 255,
        g: 215,
        b: 0,
    };
    let muted = Color::Rgb {
        r: 140,
        g: 120,
        b: 160,
    };

    // Separator line
    let sep = format!("{}─{}", ansi_fg(muted), ansi_reset());
    let sep_full = format!(
        "{}{}{}",
        sep,
        "─".repeat(width.saturating_sub(2) as usize),
        sep
    );
    queue!(out, MoveTo(x, y), Print(&sep_full), ResetColor)?;

    // Input prompt line(s)
    let prompt = format!("{}>{} ", ansi_fg(gold), ansi_reset());
    let prompt_width = 2; // "> "
    let available_width = width.saturating_sub(prompt_width + 2) as usize; // +2 for margins

    if app.input.is_empty() {
        let placeholder = format!(
            "{}Type a message or command...{}",
            ansi_fg(muted),
            ansi_reset()
        );
        let line = format!("{}{}", prompt, placeholder);
        queue!(out, MoveTo(x + 1, y + 1), Print(&line), ResetColor)?;
    } else {
        // Wrap input text across multiple lines if needed
        let input_color = ansi_fg(Color::Rgb {
            r: 220,
            g: 220,
            b: 220,
        });
        let reset = ansi_reset();

        let wrapped = wrap_input_text(&app.input, available_width);
        for (row_idx, line_text) in wrapped.iter().enumerate() {
            let row_y = y + 1 + row_idx as u16;
            if row_y >= y + height {
                break; // Safety: don't overflow the allocated area
            }
            if row_idx == 0 {
                let line = format!("{}{}{}{}", prompt, input_color, line_text, reset);
                queue!(out, MoveTo(x + 1, row_y), Print(&line), ResetColor)?;
            } else {
                // Continuation lines: indent to align with first line of text
                let indent = "  ".to_string(); // match prompt width
                let line = format!("{}{}{}{}", indent, input_color, line_text, reset);
                queue!(out, MoveTo(x + 1, row_y), Print(&line), ResetColor)?;
            }
        }
    }

    // Clear any extra rows in the input area that weren't used by wrapped text
    let used_rows = if app.input.is_empty() {
        1
    } else {
        let wrapped = wrap_input_text(&app.input, available_width);
        wrapped.len().min(height.saturating_sub(1) as usize) as u16
    };
    for row in (1 + used_rows)..height {
        let row_y = y + row;
        queue!(
            out,
            MoveTo(x + 1, row_y),
            Print(" ".repeat(width.saturating_sub(2) as usize)),
            ResetColor,
        )?;
    }

    Ok(())
}
