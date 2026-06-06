//! Mouse Support — Click, scroll, and hover in the TUI.
//!
//! Enables crossterm mouse capture for:
//! - Click to focus panes
//! - Click to place cursor in input
//! - Scroll wheel for chat history
//! - Click messages to expand/collapse

#![allow(dead_code)]

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

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
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::EnableMouseCapture
        );
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture
        );
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
    /// Ignored / not handled.
    None,
}

/// Translate a raw crossterm mouse event into a high-level action.
/// Uses approximate layout heuristics since we don't have access to
/// ratatui's layout rects outside the draw call.
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
        MouseEventKind::Drag(_) => MouseAction::DragStart { x: event.column, y: event.row },
        MouseEventKind::Up(MouseButton::Left) => MouseAction::DragEnd { x: event.column, y: event.row },
        _ => MouseAction::None,
    }
}

/// Copy the given text to the system clipboard using arboard.
pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    use arboard::Clipboard;
    let mut clipboard = Clipboard::new().map_err(|e| anyhow::anyhow!("Clipboard access failed: {}", e))?;
    clipboard.set_text(text).map_err(|e| anyhow::anyhow!("Clipboard write failed: {}", e))?;
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
    content.lines().map(|line| {
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
    }).sum()
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
            if chat_area.contains(ratatui::layout::Position::new(col, row)) {
                let relative_row = row - chat_area.y;
                MouseAction::ChatClick { y: relative_row as usize }
            } else if input_area.contains(ratatui::layout::Position::new(col, row)) {
                MouseAction::InputClick
            } else if sidebar_area.contains(ratatui::layout::Position::new(col, row)) {
                let relative_row = row - sidebar_area.y;
                MouseAction::SidebarClick { y: relative_row as usize }
            } else {
                MouseAction::None
            }
        }
        MouseEventKind::ScrollUp => {
            if chat_area.contains(ratatui::layout::Position::new(col, row)) {
                MouseAction::ScrollUp
            } else {
                MouseAction::None
            }
        }
        MouseEventKind::ScrollDown => {
            if chat_area.contains(ratatui::layout::Position::new(col, row)) {
                MouseAction::ScrollDown
            } else {
                MouseAction::None
            }
        }
        _ => MouseAction::None,
    }
}
