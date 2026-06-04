//! Session Bookmarks / Checkpoints
//!
//! Save and restore named checkpoints of the current session state.
//! Triggered via `/bookmark` command or Ctrl+B shortcut.

#![allow(dead_code)]

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};
use serde::{Deserialize, Serialize};

/// A saved checkpoint of session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    pub description: String,
    pub created_at: String,
    /// Serialized message history (simplified format).
    pub messages: Vec<BookmarkMessage>,
    /// Serialized model message history.
    pub model_messages: Vec<BookmarkModelMessage>,
    /// Which branch was active.
    pub active_branch: usize,
    /// Branch names.
    pub branches: Vec<String>,
}

/// Simplified chat message for serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

/// Simplified model message for serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkModelMessage {
    pub role: String,
    pub content: String,
}

/// Bookmark manager state.
#[derive(Debug, Clone)]
pub struct BookmarkManager {
    pub visible: bool,
    pub mode: BookmarkMode,
    pub filter: String,
    pub selected: usize,
    pub bookmarks: Vec<Bookmark>,
    pub input_name: String,
    pub input_desc: String,
    pub input_stage: usize, // 0=name, 1=desc
}

#[derive(Debug, Clone, PartialEq)]
pub enum BookmarkMode {
    List,    // Browse existing bookmarks
    Create,  // Creating a new bookmark
    Confirm, // Confirm overwrite/delete
}

impl BookmarkManager {
    pub fn new() -> Self {
        Self {
            visible: false,
            mode: BookmarkMode::List,
            filter: String::new(),
            selected: 0,
            bookmarks: Vec::new(),
            input_name: String::new(),
            input_desc: String::new(),
            input_stage: 0,
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.mode = BookmarkMode::List;
            self.filter.clear();
            self.selected = 0;
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.mode = BookmarkMode::List;
        self.filter.clear();
        self.selected = 0;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.mode = BookmarkMode::List;
    }

    pub fn start_create(&mut self) {
        self.mode = BookmarkMode::Create;
        self.input_name.clear();
        self.input_desc.clear();
        self.input_stage = 0;
    }

    pub fn type_char(&mut self, c: char) {
        match self.mode {
            BookmarkMode::List => {
                self.filter.push(c);
                self.selected = 0;
            }
            BookmarkMode::Create => {
                if self.input_stage == 0 {
                    self.input_name.push(c);
                } else {
                    self.input_desc.push(c);
                }
            }
            _ => {}
        }
    }

    pub fn backspace(&mut self) {
        match self.mode {
            BookmarkMode::List => {
                self.filter.pop();
                self.selected = self.selected.min(self.filtered().len().saturating_sub(1));
            }
            BookmarkMode::Create => {
                if self.input_stage == 0 {
                    self.input_name.pop();
                } else {
                    self.input_desc.pop();
                }
            }
            _ => {}
        }
    }

    pub fn next(&mut self) {
        if self.mode == BookmarkMode::List {
            let count = self.filtered().len();
            if count > 0 {
                self.selected = (self.selected + 1) % count;
            }
        }
    }

    pub fn prev(&mut self) {
        if self.mode == BookmarkMode::List {
            let count = self.filtered().len();
            if count > 0 {
                self.selected = self.selected.saturating_sub(1);
            }
        }
    }

    pub fn filtered(&self) -> Vec<&Bookmark> {
        let filter_lower = self.filter.to_lowercase();
        self.bookmarks
            .iter()
            .filter(|b| {
                b.name.to_lowercase().contains(&filter_lower)
                    || b.description.to_lowercase().contains(&filter_lower)
            })
            .collect()
    }

    pub fn selected_bookmark(&self) -> Option<&Bookmark> {
        let filtered = self.filtered();
        filtered.get(self.selected).copied()
    }

    pub fn advance_stage(&mut self) -> bool {
        if self.mode == BookmarkMode::Create && self.input_stage == 0 {
            self.input_stage = 1;
            false // Not done yet
        } else {
            true // Done
        }
    }

    /// Load bookmarks from the session file.
    pub fn load_from_file(&mut self, session_id: &str) {
        let path = match dirs::config_dir() {
            Some(dir) => dir.join("openshark").join("bookmarks").join(format!("{}.json", session_id)),
            None => return, // Silently skip if config dir unavailable
        };
        if let Ok(data) = std::fs::read_to_string(&path)
            && let Ok(bookmarks) = serde_json::from_str::<Vec<Bookmark>>(&data) {
                self.bookmarks = bookmarks;
            }
    }

    /// Save bookmarks to the session file.
    pub fn save_to_file(&self, session_id: &str) {
        let dir = match dirs::config_dir() {
            Some(dir) => dir.join("openshark").join("bookmarks"),
            None => return, // Silently skip if config dir unavailable
        };
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}.json", session_id));
        if let Ok(data) = serde_json::to_string_pretty(&self.bookmarks) {
            let _ = std::fs::write(&path, data);
        }
    }
}

/// Draw the bookmark manager overlay.
pub fn draw_bookmark_manager(f: &mut Frame, manager: &BookmarkManager, area: Rect) {
    if !manager.visible {
        return;
    }

    let popup_width = (area.width as f32 * 0.6) as u16;
    let popup_height = 20u16.min(area.height - 4);
    let popup_x = (area.width - popup_width) / 2;
    let popup_y = (area.height - popup_height) / 3;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    match manager.mode {
        BookmarkMode::List => draw_list_mode(f, manager, popup_area),
        BookmarkMode::Create => draw_create_mode(f, manager, popup_area),
        BookmarkMode::Confirm => draw_confirm_mode(f, manager, popup_area),
    }
}

fn draw_list_mode(f: &mut Frame, manager: &BookmarkManager, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let filter_text = if manager.filter.is_empty() {
        Text::from(Line::from(Span::styled(
            "Type to filter bookmarks...",
            Style::default().fg(Color::DarkGray),
        )))
    } else {
        Text::from(Line::from(Span::raw(&manager.filter)))
    };
    let filter_paragraph = Paragraph::new(filter_text)
        .block(Block::default().borders(Borders::ALL).title(" Bookmarks (Ctrl+N=new, Enter=load, Del=delete) "));
    f.render_widget(filter_paragraph, chunks[0]);

    let filtered = manager.filtered();
    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, bm)| {
            let is_selected = i == manager.selected;
            let name_style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Cyan)
            };
            let desc_style = if is_selected {
                Style::default().fg(Color::Gray).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Gray)
            };
            let time_style = if is_selected {
                Style::default().fg(Color::Yellow).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Yellow)
            };

            let mut spans = vec![
                Span::styled(format!(" {:20} ", bm.name), name_style),
                Span::styled(format!("{} ", bm.description), desc_style),
            ];
            spans.push(Span::styled(format!("[{}]", bm.created_at), time_style));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_widget(list, chunks[1]);
}

fn draw_create_mode(f: &mut Frame, manager: &BookmarkManager, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let name_title = if manager.input_stage == 0 {
        " Bookmark Name (*) "
    } else {
        " Bookmark Name "
    };
    let name_text = Text::from(Line::from(Span::raw(&manager.input_name)));
    let name_paragraph = Paragraph::new(name_text)
        .block(Block::default().borders(Borders::ALL).title(name_title));
    f.render_widget(name_paragraph, chunks[0]);

    let desc_title = if manager.input_stage == 1 {
        " Description (*) "
    } else {
        " Description "
    };
    let desc_text = Text::from(Line::from(Span::raw(&manager.input_desc)));
    let desc_paragraph = Paragraph::new(desc_text)
        .block(Block::default().borders(Borders::ALL).title(desc_title));
    f.render_widget(desc_paragraph, chunks[1]);

    let help = Text::from(Line::from(Span::styled(
        "Tab/Enter: next field | Esc: cancel",
        Style::default().fg(Color::DarkGray),
    )));
    let help_paragraph = Paragraph::new(help).block(Block::default().borders(Borders::ALL));
    f.render_widget(help_paragraph, chunks[2]);
}

fn draw_confirm_mode(_f: &mut Frame, _manager: &BookmarkManager, _area: Rect) {
    // Simple confirmation — handled by caller with a system message
}
