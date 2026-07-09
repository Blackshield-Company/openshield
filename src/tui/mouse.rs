//! Mouse Support — Click, scroll, and hover in the TUI.
//!
//! Enables crossterm mouse capture for:
//! - Click to focus panes
//! - Click to place cursor in input
//! - Scroll wheel for chat history
//! - Click messages to expand/collapse

#![allow(dead_code)]

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use unicode_width::UnicodeWidthChar;

use crate::tui::theme::{ansi_fg, ansi_reset};

/// Simple rectangle for hit testing.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn contains(&self, col: u16, row: u16) -> bool {
        col >= self.x && col < self.x + self.width && row >= self.y && row < self.y + self.height
    }
}

/// Mouse state tracking.
#[derive(Debug, Clone, Default)]
pub struct MouseState {
    pub enabled: bool,
    pub last_click: Option<(u16, u16)>, // (column, row)
    pub drag_start: Option<(u16, u16)>,
    pub selection_start: Option<(u16, u16)>,
    pub selection_end: Option<(u16, u16)>,
    pub selecting: bool,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            enabled: true,
            last_click: None,
            drag_start: None,
            selection_start: None,
            selection_end: None,
            selecting: false,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    }
}

/// Result of processing a mouse event.
#[derive(Debug, Clone)]
pub enum MouseAction {
    /// Clicked in the chat area — scroll to position.
    ChatClick { y: usize },
    /// Scrolled up.
    ScrollUp,
    /// Scrolled down.
    ScrollDown,
    /// Clicked in the input area — set cursor position.
    InputClick,
    /// Clicked on a sidebar item.
    SidebarClick { y: usize },
    /// Drag started.
    DragStart { x: u16, y: u16 },
    /// Drag ended.
    DragEnd { x: u16, y: u16 },
    /// Drag movement during selection.
    SelectMove { x: u16, y: u16 },
    /// Ignored / not handled.
    None,
}

/// Translate a raw crossterm mouse event into a high-level action.
/// Uses approximate layout heuristics.
pub fn translate_mouse_event(event: MouseEvent, _app: &crate::tui::App) -> MouseAction {
    let (_col, row) = (event.column, event.row);

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // Heuristic: top ~3 rows are title, bottom ~3 are input, rest is chat
            if row <= 2 {
                MouseAction::None
            } else {
                // Assume chat area; y as usize for scroll
                MouseAction::ChatClick { y: row as usize }
            }
        }
        MouseEventKind::ScrollUp => MouseAction::ScrollUp,
        MouseEventKind::ScrollDown => MouseAction::ScrollDown,
        MouseEventKind::Drag(_) => MouseAction::SelectMove {
            x: event.column,
            y: event.row,
        },
        MouseEventKind::Up(MouseButton::Left) => MouseAction::DragEnd {
            x: event.column,
            y: event.row,
        },
        _ => MouseAction::None,
    }
}

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() || ch == '\u{007f}' {
                in_escape = false;
            }
        } else if ch == '\x1b' {
            in_escape = true;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Extract the visual text actually drawn on screen from a slice of rendered
/// lines. Returns a rectangular region between `(start_col, start_row)` and
/// `(end_col, end_row)` inclusive, using visual column positions after removing
/// ANSI escapes. Rows are content-relative (i.e., starting from 0 at the top
/// of the chat area), while columns are terminal columns including the left
/// padding of the feed.
///
/// The `rendered_lines` should be the exact lines that were printed, already
/// wrapped and including ANSI escape sequences. Only the chat/message text is
/// represented; system headers are included if passed in.
pub fn extract_rectangular_text(
    rendered_lines: &[String],
    start_col: usize,
    start_row: usize,
    end_col: usize,
    end_row: usize,
    left_padding: usize,
) -> String {
    let (top, bottom) = (start_row.min(end_row), start_row.max(end_row));
    let (left, right) = (start_col.min(end_col), start_col.max(end_col));

    let mut result = String::new();
    for row in top..=bottom {
        let Some(raw) = rendered_lines.get(row) else {
            continue;
        };
        let clean = strip_ansi(raw);
        let safe_left = left.saturating_sub(left_padding);
        let safe_right = (right.saturating_sub(left_padding)).min(clean.chars().count());

        if safe_left < clean.chars().count() {
            let segment: String = clean
                .chars()
                .skip(safe_left)
                .take(safe_right.saturating_sub(safe_left))
                .collect();
            result.push_str(&segment);
        }
        if row != bottom {
            result.push('\n');
        }
    }
    result
}

/// Build the list of rendered chat lines exactly as they appear on screen.
/// This mirrors the layout in `components::chat::draw_unified_feed` so that
/// mouse selection can extract the precise visible text.
///
/// Returns `(lines, visible_scroll)` where `lines` is every line in the entire
/// virtual feed and `visible_scroll` is the clamped scroll offset used to slice
/// into the currently visible window.
pub fn build_rendered_lines(app: &crate::tui::App, width: usize) -> (Vec<String>, usize) {
    let inner_width = width.saturating_sub(2); // 1-char padding each side

    let mut all_lines: Vec<String> = Vec::new();

    // System info header (same labels as draw_unified_feed)
    all_lines.push(format_info_line("Model", &app.model, inner_width));
    all_lines.push(format_info_line(
        "Session",
        &app.session_id[..app.session_id.len().min(24)],
        inner_width,
    ));
    all_lines.push(format_info_line(
        "Ctx",
        &format!("{} / {}", app.context_used(), app.model_context_length),
        inner_width,
    ));
    all_lines.push(format_info_line(
        "Tokens",
        &app.tokens_used.to_string(),
        inner_width,
    ));
    all_lines.push(format_info_line(
        "Tools",
        &app.tool_calls_count.to_string(),
        inner_width,
    ));
    all_lines.push(format_info_line("Branch", "main", inner_width));

    // Separator
    all_lines.push(format!("{}─{}", ansi_fg(crate::tui::theme::Color::Rgb { r: 140, g: 120, b: 160 }), ansi_reset()));

    // Messages
    for (idx, msg) in app.messages.iter().enumerate() {
        let is_selected =
            app.mode == crate::tui::AppMode::CopySelect && app.copy_selected_idx == Some(idx);
        let msg_lines = format_message_for_selection(msg, inner_width, is_selected);
        all_lines.extend(msg_lines);
        all_lines.push(String::new()); // separator
    }

    // Streaming content
    if app.is_streaming && !app.streaming_content.is_empty() {
        let streaming_lines = format_streaming_for_selection(&app.streaming_content, inner_width);
        all_lines.extend(streaming_lines);
    }

    // Reasoning content
    if app.is_reasoning && !app.reasoning_content.is_empty() {
        let reasoning_lines = format_reasoning_for_selection(&app.reasoning_content, inner_width);
        all_lines.extend(reasoning_lines);
    }

    (all_lines, app.scroll)
}

fn format_info_line(label: &str, value: &str, width: usize) -> String {
    let label_color = ansi_fg(crate::tui::theme::Color::Rgb { r: 140, g: 120, b: 160 });
    let value_color = ansi_fg(crate::tui::theme::Color::Rgb { r: 220, g: 220, b: 220 });
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

/// Same as `components::chat::format_message` but without dropping text and
/// producing one rendered line per display line so the row-to-text mapping is
/// exact.
fn format_message_for_selection(
    msg: &crate::tui::ChatMessage,
    width: usize,
    is_selected: bool,
) -> Vec<String> {
    let mut lines = Vec::new();

    let (role_icon, role_color, role_name) = match msg.role.as_str() {
        "user" => (
            "👤",
            crate::tui::theme::Color::Rgb { r: 255, g: 215, b: 0 },
            "You",
        ),
        "assistant" => (
            "🦈",
            crate::tui::theme::Color::Rgb { r: 255, g: 77, b: 158 },
            "Shark",
        ),
        "system" => (
            "📋",
            crate::tui::theme::Color::Rgb { r: 140, g: 120, b: 160 },
            "System",
        ),
        _ => (
            "❓",
            crate::tui::theme::Color::Rgb { r: 220, g: 220, b: 220 },
            "Unknown",
        ),
    };

    let header = if is_selected {
        format!(
            "{}▶ {} {} {}{}{}",
            ansi_fg(crate::tui::theme::Color::Rgb { r: 255, g: 215, b: 0 }),
            role_icon,
            role_name,
            ansi_fg(crate::tui::theme::Color::Rgb { r: 140, g: 120, b: 160 }),
            " [COPY]",
            ansi_reset()
        )
    } else {
        format!(
            "{}{} {} {}{}",
            ansi_fg(role_color),
            role_icon,
            role_name,
            ansi_fg(crate::tui::theme::Color::Rgb { r: 140, g: 120, b: 160 }),
            ansi_reset()
        )
    };
    lines.push(header);

    for content_line in msg.content.lines() {
        let wrapped = wrap_line(content_line, width.saturating_sub(2));
        for w in wrapped {
            lines.push(format!(
                "{}{}{}",
                ansi_fg(crate::tui::theme::Color::Rgb { r: 220, g: 220, b: 220 }),
                w,
                ansi_reset()
            ));
        }
    }

    for response in &msg.multi_model_responses {
        lines.push(format!(
            "{}  ↳ {} ({}ms, {}tok){}",
            ansi_fg(crate::tui::theme::Color::Rgb { r: 140, g: 120, b: 160 }),
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
                    ansi_fg(crate::tui::theme::Color::Rgb { r: 140, g: 120, b: 160 }),
                    w,
                    ansi_reset()
                ));
            }
        }
    }

    lines
}

fn format_streaming_for_selection(content: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let header = format!(
        "{}🦈 Shark {}(streaming…){}",
        ansi_fg(crate::tui::theme::Color::Rgb { r: 255, g: 77, b: 158 }),
        ansi_fg(crate::tui::theme::Color::Rgb { r: 140, g: 120, b: 160 }),
        ansi_reset()
    );
    lines.push(header);

    for line in content.lines() {
        let wrapped = wrap_line(line, width.saturating_sub(2));
        for w in wrapped {
            lines.push(format!(
                "{}{}{}",
                ansi_fg(crate::tui::theme::Color::Rgb { r: 220, g: 220, b: 220 }),
                w,
                ansi_reset()
            ));
        }
    }
    lines
}

fn format_reasoning_for_selection(content: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let header = format!(
        "{}💭 Reasoning {}(thinking…){}",
        ansi_fg(crate::tui::theme::Color::Rgb { r: 140, g: 120, b: 160 }),
        ansi_fg(crate::tui::theme::Color::Rgb { r: 100, g: 100, b: 120 }),
        ansi_reset()
    );
    lines.push(header);

    for line in content.lines() {
        let wrapped = wrap_line(line, width.saturating_sub(2));
        for w in wrapped {
            lines.push(format!(
                "{}{}{}",
                ansi_fg(crate::tui::theme::Color::Rgb { r: 140, g: 140, b: 160 }),
                w,
                ansi_reset()
            ));
        }
    }
    lines
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

/// Copy the given text to the system clipboard using arboard.
pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    use arboard::Clipboard;
    let mut clipboard =
        Clipboard::new().map_err(|e| anyhow::anyhow!("Clipboard access failed: {}", e))?;
    clipboard
        .set_text(text)
        .map_err(|e| anyhow::anyhow!("Clipboard write failed: {}", e))?;
    Ok(())
}

/// Extract visible text from chat messages that would appear at the given row range.
/// This is a heuristic — we approximate which message content is at which row.
pub fn extract_selection_text(
    messages: &[crate::tui::ChatMessage],
    scroll: usize,
    start_row: usize,
    end_row: usize,
    term_width: usize,
) -> String {
    let mut result = String::new();
    let mut current_row = 0usize;

    for msg in messages.iter().skip(scroll) {
        if current_row > end_row {
            break;
        }
        let content = &msg.content;
        // Rough wrap estimate: each line is term_width chars
        let lines_needed = content.len().div_ceil(term_width.max(1));
        let msg_end_row = current_row + lines_needed;

        if msg_end_row >= start_row {
            let overlap_start = start_row.saturating_sub(current_row);
            let overlap_end = (end_row - current_row + 1).min(lines_needed);
            let start_char = overlap_start * term_width;
            let end_char = (overlap_end * term_width).min(content.len());
            if start_char < content.len() {
                result.push_str(&content[start_char..end_char.min(content.len())]);
                result.push('\n');
            }
        }
        current_row = msg_end_row + 1; // +1 for separator
    }

    result.trim_end().to_string()
}

/// Extract text from a single chat message, accounting for word wrapping.
/// Returns the substring that would appear between start_col and end_col on each row.
pub fn extract_message_text_wrapped(
    content: &str,
    start_row: usize,
    end_row: usize,
    term_width: usize,
    msg_start_row: usize,
) -> String {
    let mut result = String::new();
    let mut current_row = msg_start_row;
    let mut char_idx = 0usize;

    for line in content.lines() {
        if current_row > end_row {
            break;
        }
        // Word-wrap this line into segments of at most term_width visual columns
        let mut line_col = 0usize;
        let mut segment_start = char_idx;

        for ch in line.chars() {
            let ch_width = ch.width().unwrap_or(1);
            if line_col + ch_width > term_width && line_col > 0 {
                // Segment ends before this char
                let segment_end = char_idx;
                if current_row >= start_row && current_row <= end_row {
                    result.push_str(&line[segment_start..segment_end.min(line.len())]);
                    result.push('\n');
                }
                current_row += 1;
                if current_row > end_row {
                    break;
                }
                segment_start = char_idx;
                line_col = ch_width;
            } else {
                line_col += ch_width;
            }
            char_idx += ch.len_utf8();
        }

        // Flush remaining segment
        if current_row >= start_row && current_row <= end_row && segment_start < line.len() {
            result.push_str(&line[segment_start..line.len()]);
            result.push('\n');
        }
        current_row += 1;
        char_idx += 1; // newline
    }

    result.trim_end().to_string()
}

/// Extract selection text with proper word-wrap awareness.
pub fn extract_selection_text_wrapped(
    messages: &[crate::tui::ChatMessage],
    scroll: usize,
    start_row: usize,
    end_row: usize,
    term_width: usize,
) -> String {
    let mut result = String::new();
    let mut current_row = 0usize;

    for msg in messages.iter().skip(scroll) {
        if current_row > end_row {
            break;
        }

        let content = &msg.content;
        let lines_needed = estimate_wrapped_lines(content, term_width);
        let msg_end_row = current_row + lines_needed;

        if msg_end_row >= start_row {
            let text = extract_message_text_wrapped(
                content,
                start_row.saturating_sub(current_row),
                end_row.saturating_sub(current_row),
                term_width,
                0,
            );
            if !text.is_empty() {
                result.push_str(&text);
                result.push('\n');
            }
        }
        current_row = msg_end_row + 1; // +1 for separator
    }

    result.trim_end().to_string()
}

/// Estimate how many terminal rows a message will occupy with word wrapping.
fn estimate_wrapped_lines(content: &str, term_width: usize) -> usize {
    let width = term_width.max(1);
    content
        .lines()
        .map(|line| {
            let mut cols = 0usize;
            let mut rows = 1usize;
            for ch in line.chars() {
                let ch_width = ch.width().unwrap_or(1);
                if cols + ch_width > width {
                    rows += 1;
                    cols = ch_width;
                } else {
                    cols += ch_width;
                }
            }
            rows
        })
        .sum()
}

pub fn handle_mouse_event(
    event: MouseEvent,
    chat_area: Rect,
    input_area: Rect,
    sidebar_area: Rect,
    _title_area: Rect,
) -> MouseAction {
    let (col, row) = (event.column, event.row);

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if chat_area.contains(col, row) {
                let relative_row = row - chat_area.y;
                MouseAction::ChatClick {
                    y: relative_row as usize,
                }
            } else if input_area.contains(col, row) {
                MouseAction::InputClick
            } else if sidebar_area.contains(col, row) {
                let relative_row = row - sidebar_area.y;
                MouseAction::SidebarClick {
                    y: relative_row as usize,
                }
            } else {
                MouseAction::None
            }
        }
        MouseEventKind::ScrollUp => {
            if chat_area.contains(col, row) {
                MouseAction::ScrollUp
            } else {
                MouseAction::None
            }
        }
        MouseEventKind::ScrollDown => {
            if chat_area.contains(col, row) {
                MouseAction::ScrollDown
            } else {
                MouseAction::None
            }
        }
        _ => MouseAction::None,
    }
}
