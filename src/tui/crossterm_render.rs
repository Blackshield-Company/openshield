#![allow(dead_code)]

use crossterm::{
    cursor::{self, MoveTo, MoveToColumn, MoveToNextLine, Show},
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor, Stylize},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand, QueueableCommand,
};
use std::io::{self, stdout, Write};

/// Blackshield color palette for direct crossterm rendering.
#[derive(Debug, Clone, Copy)]
pub struct BlackshieldTheme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub accent_secondary: Color,
    pub accent_tertiary: Color,
    pub muted: Color,
    pub border: Color,
    pub border_focused: Color,
    pub error: Color,
    pub success: Color,
    pub warning: Color,
    pub selection: Color,
    pub highlight: Color,
    pub shield: Color,
}

impl Default for BlackshieldTheme {
    fn default() -> Self {
        Self::blackshield()
    }
}

impl BlackshieldTheme {
    pub const fn blackshield() -> Self {
        Self {
            bg: Color::Rgb { r: 13, g: 13, b: 17 },
            fg: Color::Rgb { r: 216, g: 211, b: 200 },
            accent: Color::Rgb { r: 193, g: 18, b: 31 },
            accent_secondary: Color::Rgb { r: 201, g: 162, b: 39 },
            accent_tertiary: Color::Rgb { r: 91, g: 127, b: 166 },
            muted: Color::Rgb { r: 138, g: 143, b: 152 },
            border: Color::Rgb { r: 26, g: 26, b: 32 },
            border_focused: Color::Rgb { r: 193, g: 18, b: 31 },
            error: Color::Rgb { r: 193, g: 18, b: 31 },
            success: Color::Rgb { r: 106, g: 153, b: 78 },
            warning: Color::Rgb { r: 201, g: 162, b: 39 },
            selection: Color::Rgb { r: 193, g: 18, b: 31 },
            highlight: Color::Rgb { r: 255, g: 107, b: 114 },
            shield: Color::Rgb { r: 193, g: 18, b: 31 },
        }
    }
}

static mut CURRENT_THEME: BlackshieldTheme = BlackshieldTheme::blackshield();

pub fn set_theme(theme: BlackshieldTheme) {
    unsafe { CURRENT_THEME = theme; }
}

pub fn current_theme() -> BlackshieldTheme {
    unsafe { CURRENT_THEME }
}

/// ANSI color helpers
pub fn ansi_fg(color: Color) -> String {
    match color {
        Color::Rgb { r, g, b } => format!("\x1b[38;2;{r};{g};{b}m"),
        Color::Black => "\x1b[30m".to_string(),
        Color::DarkGrey => "\x1b[90m".to_string(),
        Color::Red => "\x1b[91m".to_string(),
        Color::Green => "\x1b[92m".to_string(),
        Color::Yellow => "\x1b[93m".to_string(),
        Color::Blue => "\x1b[94m".to_string(),
        Color::Magenta => "\x1b[95m".to_string(),
        Color::Cyan => "\x1b[96m".to_string(),
        Color::White => "\x1b[97m".to_string(),
        _ => "\x1b[0m".to_string(),
    }
}

pub fn ansi_bg(color: Color) -> String {
    match color {
        Color::Rgb { r, g, b } => format!("\x1b[48;2;{r};{g};{b}m"),
        Color::Black => "\x1b[40m".to_string(),
        Color::DarkGrey => "\x1b[100m".to_string(),
        Color::Red => "\x1b[101m".to_string(),
        Color::Green => "\x1b[102m".to_string(),
        Color::Yellow => "\x1b[103m".to_string(),
        Color::Blue => "\x1b[104m".to_string(),
        Color::Magenta => "\x1b[105m".to_string(),
        Color::Cyan => "\x1b[106m".to_string(),
        Color::White => "\x1b[107m".to_string(),
        _ => "\x1b[0m".to_string(),
    }
}

pub fn ansi_reset() -> &'static str {
    "\x1b[0m"
}

/// Colorize a string with foreground color
pub fn colorize(text: &str, color: Color) -> String {
    format!("{}{}{}", ansi_fg(color), text, ansi_reset())
}

/// Colorize with bold
pub fn bold_colorize(text: &str, color: Color) -> String {
    format!("\x1b[1m{}{}{}", ansi_fg(color), text, ansi_reset())
}

/// Colorize with italic
pub fn italic_colorize(text: &str, color: Color) -> String {
    format!("\x1b[3m{}{}{}", ansi_fg(color), text, ansi_reset())
}

/// Terminal dimensions
pub fn terminal_size() -> io::Result<(u16, u16)> {
    terminal::size()
}

/// Clear the entire screen
pub fn clear_screen(out: &mut impl Write) -> io::Result<()> {
    out.queue(Clear(ClearType::All))?;
    out.queue(MoveTo(0, 0))?;
    out.flush()
}

/// Clear current line
pub fn clear_line(out: &mut impl Write) -> io::Result<()> {
    out.queue(Clear(ClearType::CurrentLine))?;
    out.queue(MoveToColumn(0))?;
    out.flush()
}

/// Draw a horizontal line with a color
pub fn draw_hline(width: usize, color: Color) -> String {
    colorize("─".repeat(width).as_str(), color)
}

/// Draw a box border top
pub fn draw_box_top(width: usize, color: Color) -> String {
    let inner = width.saturating_sub(2);
    format!("{}{}{}", colorize("┌", color), colorize("─".repeat(inner).as_str(), color), colorize("┐", color))
}

/// Draw a box border bottom
pub fn draw_box_bottom(width: usize, color: Color) -> String {
    let inner = width.saturating_sub(2);
    format!("{}{}{}", colorize("└", color), colorize("─".repeat(inner).as_str(), color), colorize("┘", color))
}

/// Draw a box middle line with content
pub fn draw_box_line(content: &str, width: usize, color: Color) -> String {
    let content_width = unicode_width::UnicodeWidthStr::width(content);
    let padding = width.saturating_sub(content_width + 2);
    format!("{} {}{}{}", colorize("│", color), content, " ".repeat(padding), colorize("│", color))
}

/// Center text within a width
pub fn center(text: &str, width: usize) -> String {
    let text_width = unicode_width::UnicodeWidthStr::width(text);
    if text_width >= width {
        text.to_string()
    } else {
        let padding = (width - text_width) / 2;
        format!("{}{}", " ".repeat(padding), text)
    }
}

/// Print text at a specific position
pub fn print_at(x: u16, y: u16, text: &str, out: &mut impl Write) -> io::Result<()> {
    out.queue(MoveTo(x, y))?;
    out.queue(Print(text))?;
    out.flush()
}

/// Print colored text at a position
pub fn print_colored_at(x: u16, y: u16, text: &str, color: Color, out: &mut impl Write) -> io::Result<()> {
    out.queue(MoveTo(x, y))?;
    out.queue(SetForegroundColor(color))?;
    out.queue(Print(text))?;
    out.queue(ResetColor)?;
    out.flush()
}

/// Initialize terminal for TUI mode
pub fn init_terminal() -> io::Result<()> {
    let mut out = stdout();
    out.execute(EnterAlternateScreen)?;
    out.execute(cursor::Hide)?;
    terminal::enable_raw_mode()?;
    Ok(())
}

/// Restore terminal from TUI mode
pub fn restore_terminal() -> io::Result<()> {
    let mut out = stdout();
    terminal::disable_raw_mode()?;
    out.execute(cursor::Show)?;
    out.execute(LeaveAlternateScreen)?;
    Ok(())
}
