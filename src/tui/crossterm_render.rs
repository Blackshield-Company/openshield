use crossterm::{
    cursor::{self, MoveTo, MoveToColumn, MoveToNextLine, Show},
    style::{Color, Print, ResetColor, SetForegroundColor, SetBackgroundColor, Stylize},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand, QueueableCommand,
};
use std::io::{self, stdout, Write};

/// Cyberpunk neon color palette.
#[derive(Debug, Clone, Copy)]
pub struct CyberTheme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,         // Cyan
    pub accent_secondary: Color, // Magenta/Pink
    pub accent_tertiary: Color,  // Gold/Yellow
    pub muted: Color,
    pub border: Color,         // Electric purple
    pub border_focused: Color, // Cyan
    pub error: Color,
    pub success: Color,
    pub warning: Color,
    pub selection: Color,
    pub highlight: Color,
    pub shark: Color,
}

impl Default for CyberTheme {
    fn default() -> Self {
        Self::neon_purple()
    }
}

impl CyberTheme {
    pub fn neon_purple() -> Self {
        Self {
            bg: Color::Rgb { r: 26, g: 11, b: 46 },           // #1a0b2e
            fg: Color::Rgb { r: 220, g: 220, b: 220 },        // #dcdcdc
            accent: Color::Rgb { r: 0, g: 255, b: 255 },      // #00ffff
            accent_secondary: Color::Rgb { r: 255, g: 0, b: 255 }, // #ff00ff
            accent_tertiary: Color::Rgb { r: 255, g: 215, b: 0 },  // #ffd700
            muted: Color::Rgb { r: 100, g: 80, b: 120 },      // #645078
            border: Color::Rgb { r: 138, g: 43, b: 226 },     // #8a2be2
            border_focused: Color::Rgb { r: 0, g: 255, b: 255 }, // #00ffff
            error: Color::Rgb { r: 255, g: 80, b: 80 },       // #ff5050
            success: Color::Rgb { r: 80, g: 255, b: 120 },    // #50ff78
            warning: Color::Rgb { r: 255, g: 200, b: 60 },    // #ffc83c
            selection: Color::Rgb { r: 60, g: 30, b: 80 },    // #3c1e50
            highlight: Color::Rgb { r: 255, g: 0, b: 255 },    // #ff00ff
            shark: Color::Rgb { r: 255, g: 0, b: 255 },       // #ff00ff
        }
    }
}

static mut CURRENT_THEME: CyberTheme = CyberTheme::neon_purple();

pub fn set_theme(theme: CyberTheme) {
    unsafe { CURRENT_THEME = theme; }
}

pub fn current_theme() -> CyberTheme {
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
