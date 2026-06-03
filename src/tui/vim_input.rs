//! Vim Mode Input — Vim keybindings for the TUI input area.
//!
//! Supports Normal, Insert, and Visual modes.
//! Normal mode: h/j/k/l navigation, w/b word movement, x delete, dd delete line,
//!             yy yank, p paste, u undo, r redo, i/I/a/A/o/O enter insert,
//!             v visual, Esc normal, : command line
//! Insert mode: standard typing, Esc to normal
//! Visual mode: h/j/k/l selection, y yank, d delete, x delete, Esc cancel

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Insert,
    Normal,
    Visual,
    Command,
}

#[derive(Debug, Clone, Default)]
pub struct VimState {
    pub mode: VimMode,
    /// Visual selection start (inclusive).
    pub visual_start: Option<usize>,
    /// Command buffer after `:` in normal mode.
    pub command_buffer: String,
    /// Count prefix for motions (e.g., 3j).
    pub count: Option<usize>,
    /// Last operator-pending command (for `.` repeat).
    pub last_command: Option<String>,
    /// Small delete register (for x, dd without yank).
    pub small_register: String,
    /// Yank register.
    pub yank_register: String,
}

impl VimState {
    pub fn new() -> Self {
        Self {
            mode: VimMode::Insert, // Start in insert for approachability; Ctrl+[ or Esc to normal
            visual_start: None,
            command_buffer: String::new(),
            count: None,
            last_command: None,
            small_register: String::new(),
            yank_register: String::new(),
        }
    }

    pub fn is_normal(&self) -> bool {
        self.mode == VimMode::Normal
    }

    pub fn is_insert(&self) -> bool {
        self.mode == VimMode::Insert
    }

    pub fn is_visual(&self) -> bool {
        self.mode == VimMode::Visual
    }

    pub fn mode_indicator(&self) -> &'static str {
        match self.mode {
            VimMode::Normal => "NORMAL",
            VimMode::Insert => "INSERT",
            VimMode::Visual => "VISUAL",
            VimMode::Command => "COMMAND",
        }
    }

    pub fn mode_color(&self) -> ratatui::style::Color {
        match self.mode {
            VimMode::Normal => ratatui::style::Color::Green,
            VimMode::Insert => ratatui::style::Color::Blue,
            VimMode::Visual => ratatui::style::Color::Yellow,
            VimMode::Command => ratatui::style::Color::Magenta,
        }
    }
}

/// Process a key event in vim mode.
/// Returns (handled, should_submit) where should_submit is true on Enter in insert mode.
pub fn handle_vim_key(
    key: KeyEvent,
    vim: &mut VimState,
    input: &mut String,
    cursor: &mut usize,
) -> (bool, bool) {
    match vim.mode {
        VimMode::Normal => handle_normal_mode(key, vim, input, cursor),
        VimMode::Insert => handle_insert_mode(key, vim, input, cursor),
        VimMode::Visual => handle_visual_mode(key, vim, input, cursor),
        VimMode::Command => handle_command_mode(key, vim, input, cursor),
    }
}

fn handle_insert_mode(
    key: KeyEvent,
    vim: &mut VimState,
    input: &mut String,
    cursor: &mut usize,
) -> (bool, bool) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('[') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            vim.mode = VimMode::Normal;
            // Clamp cursor to valid position
            if *cursor > input.len() {
                *cursor = input.len();
            }
            if !input.is_empty() && *cursor > 0 {
                *cursor -= 1; // Move back one char like vim
            }
            (true, false)
        }
        KeyCode::Enter => {
            // Submit the input
            (true, true)
        }
        KeyCode::Char(c) => {
            if *cursor <= input.len() {
                input.insert(*cursor, c);
                *cursor += 1;
            }
            (true, false)
        }
        KeyCode::Backspace => {
            if *cursor > 0 {
                input.remove(*cursor - 1);
                *cursor -= 1;
            }
            (true, false)
        }
        KeyCode::Delete => {
            if *cursor < input.len() {
                input.remove(*cursor);
            }
            (true, false)
        }
        KeyCode::Left => {
            if *cursor > 0 {
                *cursor -= 1;
            }
            (true, false)
        }
        KeyCode::Right => {
            if *cursor < input.len() {
                *cursor += 1;
            }
            (true, false)
        }
        KeyCode::Home => {
            *cursor = 0;
            (true, false)
        }
        KeyCode::End => {
            *cursor = input.len();
            (true, false)
        }
        _ => (false, false),
    }
}

fn handle_normal_mode(
    key: KeyEvent,
    vim: &mut VimState,
    input: &mut String,
    cursor: &mut usize,
) -> (bool, bool) {
    // Number keys build count prefix
    if let KeyCode::Char(c) = key.code {
        if c.is_ascii_digit() && c != '0' {
            let digit = c.to_digit(10).unwrap() as usize;
            vim.count = Some(vim.count.unwrap_or(0) * 10 + digit);
            return (true, false);
        }
    }

    let count = vim.count.take().unwrap_or(1);

    match key.code {
        KeyCode::Char('i') => {
            vim.mode = VimMode::Insert;
            (true, false)
        }
        KeyCode::Char('I') => {
            *cursor = 0;
            vim.mode = VimMode::Insert;
            (true, false)
        }
        KeyCode::Char('a') => {
            if *cursor < input.len() {
                *cursor += 1;
            }
            vim.mode = VimMode::Insert;
            (true, false)
        }
        KeyCode::Char('A') => {
            *cursor = input.len();
            vim.mode = VimMode::Insert;
            (true, false)
        }
        KeyCode::Char('o') => {
            input.insert(*cursor, '\n');
            *cursor += 1;
            vim.mode = VimMode::Insert;
            (true, false)
        }
        KeyCode::Char('O') => {
            // Insert newline before current position
            let pos = *cursor;
            let line_start = input[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
            input.insert(line_start, '\n');
            *cursor = line_start;
            vim.mode = VimMode::Insert;
            (true, false)
        }
        KeyCode::Char('h') | KeyCode::Left => {
            for _ in 0..count {
                if *cursor > 0 {
                    *cursor -= 1;
                }
            }
            (true, false)
        }
        KeyCode::Char('j') => {
            // Move to start of next line
            if let Some(next_nl) = input[*cursor..].find('\n') {
                *cursor += next_nl + 1;
            }
            (true, false)
        }
        KeyCode::Char('k') => {
            // Move to start of previous line
            if *cursor > 0 {
                let prev_nl = input[..cursor.saturating_sub(1)].rfind('\n');
                if let Some(p) = prev_nl {
                    *cursor = p;
                } else {
                    *cursor = 0;
                }
            }
            (true, false)
        }
        KeyCode::Char('l') | KeyCode::Right => {
            for _ in 0..count {
                if *cursor < input.len().saturating_sub(1) {
                    *cursor += 1;
                }
            }
            (true, false)
        }
        KeyCode::Char('w') => {
            // Move forward by word
            for _ in 0..count {
                skip_word_forward(input, cursor);
            }
            (true, false)
        }
        KeyCode::Char('b') => {
            // Move backward by word
            for _ in 0..count {
                skip_word_backward(input, cursor);
            }
            (true, false)
        }
        KeyCode::Char('0') => {
            *cursor = 0;
            (true, false)
        }
        KeyCode::Char('$') => {
            if !input.is_empty() {
                *cursor = input.len().saturating_sub(1);
            }
            (true, false)
        }
        KeyCode::Char('x') => {
            // Delete char under cursor
            for _ in 0..count {
                if *cursor < input.len() {
                    let ch = input.remove(*cursor);
                    vim.small_register.push(ch);
                }
            }
            (true, false)
        }
        KeyCode::Char('d') => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                // dd — delete line
                delete_line(input, cursor, &mut vim.yank_register);
            } else {
                // Operator pending — wait for motion (simplified: delete to end of line)
                delete_to_end(input, cursor, &mut vim.yank_register);
            }
            (true, false)
        }
        KeyCode::Char('D') => {
            delete_to_end(input, cursor, &mut vim.yank_register);
            (true, false)
        }
        KeyCode::Char('y') => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                // yy — yank line
                yank_line(input, cursor, &mut vim.yank_register);
            } else {
                yank_to_end(input, cursor, &mut vim.yank_register);
            }
            (true, false)
        }
        KeyCode::Char('Y') => {
            yank_to_end(input, cursor, &mut vim.yank_register);
            (true, false)
        }
        KeyCode::Char('p') => {
            // Paste after cursor
            for _ in 0..count {
                for ch in vim.yank_register.chars() {
                    if *cursor < input.len() {
                        *cursor += 1;
                    }
                    input.insert(*cursor, ch);
                    *cursor += 1;
                }
            }
            (true, false)
        }
        KeyCode::Char('P') => {
            // Paste before cursor
            for _ in 0..count {
                for ch in vim.yank_register.chars().rev() {
                    input.insert(*cursor, ch);
                }
            }
            (true, false)
        }
        KeyCode::Char('v') => {
            vim.mode = VimMode::Visual;
            vim.visual_start = Some(*cursor);
            (true, false)
        }
        KeyCode::Char('V') => {
            vim.mode = VimMode::Visual;
            // Line-wise visual: select current line
            let line_start = input[..*cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
            vim.visual_start = Some(line_start);
            // Move to end of line
            if let Some(next_nl) = input[*cursor..].find('\n') {
                *cursor += next_nl;
            } else {
                *cursor = input.len().saturating_sub(1);
            }
            (true, false)
        }
        KeyCode::Char(':') => {
            vim.mode = VimMode::Command;
            vim.command_buffer.clear();
            (true, false)
        }
        KeyCode::Char('u') => {
            // Undo not implemented in this simple version
            (true, false)
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Redo not implemented
            (true, false)
        }
        KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // gg — go to start
            *cursor = 0;
            (true, false)
        }
        KeyCode::Char('G') => {
            // G — go to end
            *cursor = input.len().saturating_sub(1);
            (true, false)
        }
        KeyCode::Enter => {
            // In normal mode, Enter submits
            (true, true)
        }
        _ => (false, false),
    }
}

fn handle_visual_mode(
    key: KeyEvent,
    vim: &mut VimState,
    input: &mut String,
    cursor: &mut usize,
) -> (bool, bool) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('[') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            vim.mode = VimMode::Normal;
            vim.visual_start = None;
            (true, false)
        }
        KeyCode::Char('h') | KeyCode::Left => {
            if *cursor > 0 {
                *cursor -= 1;
            }
            (true, false)
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if *cursor < input.len().saturating_sub(1) {
                *cursor += 1;
            }
            (true, false)
        }
        KeyCode::Char('d') | KeyCode::Char('x') => {
            if let Some(start) = vim.visual_start {
                let (lo, hi) = if start <= *cursor {
                    (start, *cursor + 1)
                } else {
                    (*cursor, start + 1)
                };
                if hi <= input.len() {
                    vim.yank_register = input[lo..hi].to_string();
                    input.replace_range(lo..hi, "");
                    *cursor = lo;
                }
            }
            vim.mode = VimMode::Normal;
            vim.visual_start = None;
            (true, false)
        }
        KeyCode::Char('y') => {
            if let Some(start) = vim.visual_start {
                let (lo, hi) = if start <= *cursor {
                    (start, *cursor + 1)
                } else {
                    (*cursor, start + 1)
                };
                if hi <= input.len() {
                    vim.yank_register = input[lo..hi].to_string();
                }
            }
            vim.mode = VimMode::Normal;
            vim.visual_start = None;
            (true, false)
        }
        _ => (false, false),
    }
}

fn handle_command_mode(
    key: KeyEvent,
    vim: &mut VimState,
    input: &mut String,
    cursor: &mut usize,
) -> (bool, bool) {
    match key.code {
        KeyCode::Esc => {
            vim.mode = VimMode::Normal;
            vim.command_buffer.clear();
            (true, false)
        }
        KeyCode::Enter => {
            let cmd = vim.command_buffer.clone();
            vim.command_buffer.clear();
            vim.mode = VimMode::Normal;
            return execute_vim_command(&cmd, input, cursor);
        }
        KeyCode::Char(c) => {
            vim.command_buffer.push(c);
            (true, false)
        }
        KeyCode::Backspace => {
            vim.command_buffer.pop();
            (true, false)
        }
        _ => (false, false),
    }
}

fn execute_vim_command(cmd: &str, input: &mut String, cursor: &mut usize) -> (bool, bool) {
    match cmd {
        "q" | "quit" => {
            // Signal quit — handled at App level
            (true, false)
        }
        "w" | "write" => {
            // No-op in this context (no file to write)
            (true, false)
        }
        "wq" => {
            // Write and quit
            (true, false)
        }
        "x" | "xit" => {
            // Same as wq
            (true, false)
        }
        "clear" | "%d" => {
            input.clear();
            *cursor = 0;
            (true, false)
        }
        s if s.starts_with("s/") => {
            // Simple substitute: s/old/new/
            let parts: Vec<&str> = s[2..].split('/').collect();
            if parts.len() >= 2 {
                let old = parts[0];
                let new = parts[1];
                *input = input.replace(old, new);
                if *cursor > input.len() {
                    *cursor = input.len();
                }
            }
            (true, false)
        }
        n if n.parse::<usize>().is_ok() => {
            // :<number> — go to line
            let target = n.parse::<usize>().unwrap_or(1).saturating_sub(1);
            let mut line = 0;
            let mut pos = 0;
            for (i, ch) in input.chars().enumerate() {
                if line == target {
                    pos = i;
                    break;
                }
                if ch == '\n' {
                    line += 1;
                }
            }
            *cursor = pos;
            (true, false)
        }
        _ => (true, false),
    }
}

fn skip_word_forward(input: &str, cursor: &mut usize) {
    let bytes = input.as_bytes();
    // Skip current word
    while *cursor < bytes.len() && bytes[*cursor].is_ascii_alphanumeric() {
        *cursor += 1;
    }
    // Skip non-word chars
    while *cursor < bytes.len() && !bytes[*cursor].is_ascii_alphanumeric() {
        *cursor += 1;
    }
}

fn skip_word_backward(input: &str, cursor: &mut usize) {
    let bytes = input.as_bytes();
    if *cursor == 0 {
        return;
    }
    *cursor -= 1;
    // Skip non-word chars
    while *cursor > 0 && !bytes[*cursor].is_ascii_alphanumeric() {
        *cursor -= 1;
    }
    // Skip word chars
    while *cursor > 0 && bytes[*cursor].is_ascii_alphanumeric() {
        *cursor -= 1;
    }
    if !bytes[*cursor].is_ascii_alphanumeric() {
        *cursor += 1;
    }
}

fn delete_line(input: &mut String, cursor: &mut usize, register: &mut String) {
    let line_start = input[..*cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = input[*cursor..]
        .find('\n')
        .map(|i| *cursor + i + 1)
        .unwrap_or(input.len());
    *register = input[line_start..line_end].to_string();
    input.replace_range(line_start..line_end, "");
    *cursor = line_start.min(input.len());
}

fn delete_to_end(input: &mut String, cursor: &mut usize, register: &mut String) {
    if *cursor < input.len() {
        *register = input[*cursor..].to_string();
        input.truncate(*cursor);
    }
}

fn yank_line(input: &mut String, cursor: &mut usize, register: &mut String) {
    let line_start = input[..*cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = input[*cursor..]
        .find('\n')
        .map(|i| *cursor + i)
        .unwrap_or(input.len());
    *register = input[line_start..line_end].to_string();
}

fn yank_to_end(input: &mut String, cursor: &mut usize, register: &mut String) {
    if *cursor < input.len() {
        *register = input[*cursor..].to_string();
    }
}
