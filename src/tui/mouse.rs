//! Mouse Support — Click, scroll, and hover in the TUI.
//!
//! Enables crossterm mouse capture for:
//! - Click to focus panes
//! - Click to place cursor in input
//! - Scroll wheel for chat history
//! - Click messages to expand/collapse

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

/// Mouse state tracking.
#[derive(Debug, Clone, Default)]
pub struct MouseState {
    pub enabled: bool,
    pub last_click: Option<(u16, u16)>, // (column, row)
    pub drag_start: Option<(u16, u16)>,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            enabled: true,
            last_click: None,
            drag_start: None,
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

/// Process a mouse event and determine the action.
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
