//! Command Palette — fuzzy-searchable slash commands
//!
//! Triggered by `/` in the input box or `Ctrl+P` anywhere.
//! Shows all available commands with descriptions, filtered by fuzzy match.

#![allow(dead_code)]

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

/// A single command entry in the palette.
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub name: String,
    pub description: String,
    pub shortcut: Option<String>,
}

/// The command palette state.
#[derive(Debug, Clone)]
pub struct CommandPalette {
    pub visible: bool,
    pub filter: String,
    pub selected: usize,
    pub commands: Vec<CommandEntry>,
}

impl CommandPalette {
    pub fn new() -> Self {
        let commands = vec![
            CommandEntry { name: "/clear".to_string(), description: "Clear the chat history".to_string(), shortcut: Some("Ctrl+L".to_string()) },
            CommandEntry { name: "/export".to_string(), description: "Export session to JSON".to_string(), shortcut: None },
            CommandEntry { name: "/import".to_string(), description: "Import session from JSON".to_string(), shortcut: None },
            CommandEntry { name: "/imports".to_string(), description: "List available imports".to_string(), shortcut: None },
            CommandEntry { name: "/diff".to_string(), description: "Show diff preview help".to_string(), shortcut: None },
            CommandEntry { name: "/run".to_string(), description: "Execute code from last response".to_string(), shortcut: None },
            CommandEntry { name: "/git".to_string(), description: "Git operations (status, diff, log, etc.)".to_string(), shortcut: None },
            CommandEntry { name: "/search".to_string(), description: "Web search".to_string(), shortcut: None },
            CommandEntry { name: "/compare".to_string(), description: "Show multi-model comparison".to_string(), shortcut: None },
            CommandEntry { name: "/multi".to_string(), description: "Toggle multi-model mode".to_string(), shortcut: None },
            CommandEntry { name: "/swarm".to_string(), description: "Swarm mode commands".to_string(), shortcut: Some("Ctrl+W".to_string()) },
            CommandEntry { name: "/undo".to_string(), description: "Undo last file edit".to_string(), shortcut: None },
            CommandEntry { name: "/model".to_string(), description: "Switch model".to_string(), shortcut: None },
            CommandEntry { name: "/theme".to_string(), description: "Change TUI theme".to_string(), shortcut: None },
            CommandEntry { name: "/help".to_string(), description: "Show help".to_string(), shortcut: Some("Ctrl+H".to_string()) },
            CommandEntry { name: "/quit".to_string(), description: "Quit OpenShark".to_string(), shortcut: Some("Ctrl+Q".to_string()) },
        ];
        Self { visible: false, filter: String::new(), selected: 0, commands }
    }

    /// Toggle visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.filter.clear();
            self.selected = 0;
        }
    }

    /// Show the palette.
    pub fn show(&mut self) {
        self.visible = true;
        self.filter.clear();
        self.selected = 0;
    }

    /// Hide the palette.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Add a character to the filter.
    pub fn type_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
    }

    /// Backspace in the filter.
    pub fn backspace(&mut self) {
        self.filter.pop();
        self.selected = self.selected.min(self.filtered().len().saturating_sub(1));
    }

    /// Move selection down.
    pub fn next(&mut self) {
        let count = self.filtered().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    /// Move selection up.
    pub fn prev(&mut self) {
        let count = self.filtered().len();
        if count > 0 {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    /// Get filtered commands.
    pub fn filtered(&self) -> Vec<&CommandEntry> {
        let filter_lower = self.filter.to_lowercase();
        self.commands
            .iter()
            .filter(|cmd| {
                cmd.name.to_lowercase().contains(&filter_lower)
                    || cmd.description.to_lowercase().contains(&filter_lower)
            })
            .collect()
    }

    /// Get the currently selected command name, if any.
    pub fn selected_command(&self) -> Option<String> {
        let filtered = self.filtered();
        filtered.get(self.selected).map(|cmd| cmd.name.clone())
    }
}

/// Draw the command palette overlay.
pub fn draw_command_palette(f: &mut Frame, palette: &CommandPalette, area: Rect) {
    if !palette.visible {
        return;
    }

    // Centered popup — 60% width, up to 20 lines tall
    let popup_width = (area.width as f32 * 0.6) as u16;
    let popup_height = 20u16.min(area.height - 4);
    let popup_x = (area.width - popup_width) / 2;
    let popup_y = (area.height - popup_height) / 3; // Upper third
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear background
    f.render_widget(Clear, popup_area);

    // Split into filter input + list
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(popup_area);

    // Filter input box
    let filter_text = if palette.filter.is_empty() {
        Text::from(Line::from(Span::styled(
            "Type to filter commands...",
            Style::default().fg(Color::DarkGray),
        )))
    } else {
        Text::from(Line::from(Span::raw(&palette.filter)))
    };
    let filter_paragraph = Paragraph::new(filter_text)
        .block(Block::default().borders(Borders::ALL).title(" Command Palette "));
    f.render_widget(filter_paragraph, chunks[0]);

    // Command list
    let filtered = palette.filtered();
    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let is_selected = i == palette.selected;
            let name_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Cyan)
            };
            let desc_style = if is_selected {
                Style::default().fg(Color::Gray).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Gray)
            };
            let shortcut_style = if is_selected {
                Style::default().fg(Color::Yellow).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Yellow)
            };

            let mut spans = vec![
                Span::styled(format!(" {:12} ", cmd.name), name_style),
                Span::styled(cmd.description.clone(), desc_style),
            ];
            if let Some(ref shortcut) = cmd.shortcut {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(format!("[{}]", shortcut), shortcut_style));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_widget(list, chunks[1]);
}
