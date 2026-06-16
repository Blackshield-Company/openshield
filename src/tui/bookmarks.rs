//! Session Bookmarks / Checkpoints
//!
//! Save and restore named checkpoints of the current session state.
//! Triggered via `/bookmark` command or Ctrl+B shortcut.

#![allow(dead_code)]

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
            Some(dir) => dir
                .join("openshark")
                .join("bookmarks")
                .join(format!("{}.json", session_id)),
            None => return, // Silently skip if config dir unavailable
        };
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(bookmarks) = serde_json::from_str::<Vec<Bookmark>>(&data) {
                self.bookmarks = bookmarks;
            }
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
