//! Command Palette — fuzzy-searchable slash commands
//!
//! Triggered by `/` in the input box or `Ctrl+P` anywhere.
//! Shows all available commands with descriptions, filtered by fuzzy match.

#![allow(dead_code)]

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
            CommandEntry {
                name: "/clear".to_string(),
                description: "Clear the chat history".to_string(),
                shortcut: Some("Ctrl+L".to_string()),
            },
            CommandEntry {
                name: "/export".to_string(),
                description: "Export session to JSON".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/import".to_string(),
                description: "Import session from JSON".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/imports".to_string(),
                description: "List available imports".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/diff".to_string(),
                description: "Show diff preview help".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/run".to_string(),
                description: "Execute code from last response".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/git".to_string(),
                description: "Git operations (status, diff, log, etc.)".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/search".to_string(),
                description: "Web search".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/compare".to_string(),
                description: "Show multi-model comparison".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/multi".to_string(),
                description: "Toggle multi-model mode".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/swarm".to_string(),
                description: "Swarm mode commands".to_string(),
                shortcut: Some("Ctrl+W".to_string()),
            },
            CommandEntry {
                name: "/undo".to_string(),
                description: "Undo last file edit".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/model".to_string(),
                description: "Switch model".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/agent".to_string(),
                description: "Switch agent persona".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/soul".to_string(),
                description: "Display current agent's full persona".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/theme".to_string(),
                description: "Change TUI theme".to_string(),
                shortcut: None,
            },
            CommandEntry {
                name: "/help".to_string(),
                description: "Show help".to_string(),
                shortcut: Some("Ctrl+H".to_string()),
            },
            CommandEntry {
                name: "/quit".to_string(),
                description: "Quit OpenShark".to_string(),
                shortcut: Some("Ctrl+Q".to_string()),
            },
        ];
        Self {
            visible: false,
            filter: String::new(),
            selected: 0,
            commands,
        }
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
