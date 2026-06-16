/// Direct ANSI rendering for the chat area and input bar.
/// Refactored to match Hermes/Claw-Code TUI style:
/// - Gold-bordered boxed chat area with centered title
/// - Cleaner message formatting with role icons and color-coded headers
/// - Status bar style input bar with model info, progress, and timer
/// - Bottom status bar like Hermes: model | ctx | [████░░░░░░] | 38s | [OK]
use std::io::{self, stdout, Write};

use crossterm::{
    cursor::MoveTo,
    style::{Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
    queue,
};

use crate::tui::theme::*;
use crate::tui::{App, ChatMessage};

/// Draw the main chat area with messages using direct ANSI output.
/// `area` is (x, y, width, height) in terminal coordinates.
pub fn draw_chat_area(app: &App, area: (u16, u16, u16, u16)) -> io::Result<()> {
    let (x, y, width, height) = area;
    let mut out = stdout();

    let border_color = if app.focused_pane == 1 {
        current_theme().border_focused
    } else {
        current_theme().border
    };
    let gold = Color::Rgb { r: 255, g: 215, b: 0 };

    // Top border with centered title
    queue!(out, MoveTo(x, y), SetForegroundColor(border_color))?;
    queue!(out, Print("┌"), Print("─".repeat((width - 2) as usize)), Print("┐"))?;
    queue!(out, ResetColor)?;

    let title = " Chat ";
    let title_x = x + (width - title.len() as u16) / 2;
    queue!(out, MoveTo(title_x, y), SetForegroundColor(gold), Print(title), ResetColor)?;

    // Side borders
    for row in 1..height - 1 {
        queue!(out, MoveTo(x, y + row), SetForegroundColor(border_color), Print("│"), ResetColor)?;
        queue!(out, MoveTo(x + width - 1, y + row), SetForegroundColor(border_color), Print("│"), ResetColor)?;
    }

    // Bottom border
    queue!(out, MoveTo(x, y + height - 1), SetForegroundColor(border_color))?;
    queue!(out, Print("└"), Print("─".repeat((width - 2) as usize)), Print("┘"))?;
    queue!(out, ResetColor)?;

    // Content area (inside borders)
    let inner_x = x + 1;
    let inner_y = y + 1;
    let inner_width = width.saturating_sub(2);
    let inner_height = height.saturating_sub(2) as usize;

    // Calculate visible messages based on scroll
    let total_messages = app.messages.len();
    let scroll = app.scroll.min(total_messages.saturating_sub(inner_height));

    let mut current_row = 0u16;
    for msg in app.messages.iter().skip(scroll).take(inner_height) {
        let msg_lines = format_message(msg, inner_width as usize);
        for line in msg_lines {
            if current_row >= inner_height as u16 {
                break;
            }
            queue!(out, MoveTo(inner_x, inner_y + current_row), Print(&line), Clear(ClearType::UntilNewLine))?;
            current_row += 1;
        }
        // Separator line between messages
        if current_row < inner_height as u16 {
            queue!(out, MoveTo(inner_x, inner_y + current_row), Clear(ClearType::UntilNewLine))?;
            current_row += 1;
        }
    }

    // Clear remaining rows
    for row in current_row..inner_height as u16 {
        queue!(out, MoveTo(inner_x, inner_y + row), Clear(ClearType::UntilNewLine))?;
    }

    out.flush()
}

/// Format a single chat message into displayable strings with ANSI colors.
fn format_message(msg: &ChatMessage, width: usize) -> Vec<String> {
    let mut lines = Vec::new();

    let (role_icon, role_color, role_name) = match msg.role.as_str() {
        "user" => ("👤", Color::Rgb { r: 255, g: 215, b: 0 }, "You"),       // Gold
        "assistant" => ("🦞", Color::Rgb { r: 255, g: 77, b: 158 }, "Shark"), // Pink
        "system" => ("📋", Color::Rgb { r: 140, g: 120, b: 160 }, "System"),  // Muted
        _ => ("❓", Color::Rgb { r: 220, g: 220, b: 220 }, "Unknown"),
    };

    // Role header with icon and name
    let header = format!(
        "{}{} {} {}{}",
        ansi_fg(role_color),
        role_icon,
        role_name,
        ansi_fg(Color::Rgb { r: 140, g: 120, b: 160 }),
        ansi_reset()
    );
    lines.push(header);

    // Content lines (word-wrapped)
    for content_line in msg.content.lines() {
        let wrapped = wrap_line(content_line, width.saturating_sub(2));
        for w in wrapped {
            lines.push(format!(
                "{}{}{}",
                ansi_fg(Color::Rgb { r: 220, g: 220, b: 220 }),
                w,
                ansi_reset()
            ));
        }
    }

    // Multi-model responses
    for response in &msg.multi_model_responses {
        lines.push(format!(
            "{}  ↳ {} ({}ms, {}tok){}",
            ansi_fg(Color::Rgb { r: 140, g: 120, b: 160 }),
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
                    ansi_fg(Color::Rgb { r: 140, g: 120, b: 160 }),
                    w,
                    ansi_reset()
                ));
            }
        }
    }

    lines
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

/// Draw the input bar at the bottom — styled like a status bar.
/// Hermes style: model | ctx -- | [████░░░░░░] | 38s | [OK]
pub fn draw_input_bar(app: &App, area: (u16, u16, u16, u16)) -> io::Result<()> {
    let (x, y, width, _height) = area;
    let mut out = stdout();

    let gold = Color::Rgb { r: 255, g: 215, b: 0 };
    let cyan = Color::Rgb { r: 0, g: 255, b: 255 };
    let green = Color::Rgb { r: 80, g: 255, b: 120 };
    let muted = Color::Rgb { r: 140, g: 120, b: 160 };

    // Build status bar string
    let model_short = app.model.split('/').last().unwrap_or(&app.model);
    let model_part = format!("{}{}{}", ansi_fg(cyan), model_short, ansi_reset());

    let ctx_part = format!("{}ctx --{}", ansi_fg(muted), ansi_reset());

    let progress = if app.is_streaming {
        let elapsed = app.stream_start_time.map(|s| s.elapsed().as_secs()).unwrap_or(0);
        let bars = (elapsed % 10) as usize;
        let filled = "█".repeat(bars);
        let empty = "░".repeat(10 - bars);
        let elapsed_str = format_elapsed(app.stream_start_time);
        format!(
            "{}[{}{}]{} {}",
            ansi_fg(green),
            filled,
            empty,
            ansi_reset(),
            elapsed_str
        )
    } else {
        format!("{}[░░░░░░░░░░]{} --", ansi_fg(muted), ansi_reset())
    };

    let status = if app.is_streaming {
        format!("{}[STREAM]{}", ansi_fg(gold), ansi_reset())
    } else {
        format!("{}[OK]{}", ansi_fg(green), ansi_reset())
    };

    let status_bar = format!("{} | {} | {} | {}", model_part, ctx_part, progress, status);

    // Draw the status bar line at the top of the input area
    queue!(out, MoveTo(x, y), SetForegroundColor(gold))?;
    queue!(out, Print("┌"), Print("─".repeat((width - 2) as usize)), Print("┐"))?;
    queue!(out, ResetColor)?;

    // Status bar content
    let status_x = x + 1;
    queue!(out, MoveTo(status_x, y), Print(&status_bar), Clear(ClearType::UntilNewLine), ResetColor)?;

    // Input prompt line
    let prompt_y = y + 1;
    let prompt = format!("{}>{}", ansi_fg(gold), ansi_reset());
    let display_input = if app.input.is_empty() {
        format!(
            "{} {}Type a message or command...{}",
            prompt,
            ansi_fg(muted),
            ansi_reset()
        )
    } else {
        format!("{} {}{}", prompt, ansi_fg(Color::Rgb { r: 220, g: 220, b: 220 }), &app.input)
    };

    queue!(out, MoveTo(x, prompt_y), SetForegroundColor(gold), Print("│"), ResetColor)?;
    queue!(out, MoveTo(x + 1, prompt_y), Print(&display_input), Clear(ClearType::UntilNewLine), ResetColor)?;
    queue!(out, MoveTo(x + width - 1, prompt_y), SetForegroundColor(gold), Print("│"), ResetColor)?;

    // Bottom border
    let bottom_y = y + 2;
    queue!(out, MoveTo(x, bottom_y), SetForegroundColor(gold))?;
    queue!(out, Print("└"), Print("─".repeat((width - 2) as usize)), Print("┘"))?;
    queue!(out, ResetColor)?;

    out.flush()
}

fn format_elapsed(start: Option<std::time::Instant>) -> String {
    match start {
        Some(s) => {
            let secs = s.elapsed().as_secs();
            format!("{}s", secs)
        }
        None => "--".to_string(),
    }
}
