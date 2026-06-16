#![allow(dead_code)]

/// Cyberpunk neon theme for OpenShark.
/// Deep purple background with gold/yellow accents — inspired by Hermes TUI.
///
/// This module uses raw ANSI color codes (via crossterm) instead of ratatui's
/// Style system. Colors are represented as crossterm::style::Color for direct
/// terminal output.
pub use crossterm::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,         // Cyan — primary highlights
    pub accent_secondary: Color, // Gold/Yellow — wordmark, important text
    pub accent_tertiary: Color,  // Pink/Magenta — secondary highlights
    pub muted: Color,
    pub border: Color,
    pub border_focused: Color,
    pub error: Color,
    pub success: Color,
    pub warning: Color,
    pub selection: Color,
    pub highlight: Color,
    pub shark: Color,          // Gold for the shark branding
}

impl Default for Theme {
    fn default() -> Self {
        Self::neon_purple()
    }
}

impl Theme {
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "neon_purple" | "default" => Some(Self::neon_purple()),
            "dark_ocean" => Some(Self::dark_ocean()),
            "high_contrast" => Some(Self::high_contrast()),
            _ => None,
        }
    }

    pub fn names() -> Vec<&'static str> {
        vec!["neon_purple", "dark_ocean", "high_contrast"]
    }

    pub fn name(&self) -> &'static str {
        "neon_purple"
    }

    pub fn border_unfocused(&self) -> Color {
        self.border
    }

    pub const fn neon_purple() -> Self {
        Self {
            // OpenShark true colors — deep purple background
            bg: Color::Rgb { r: 45, g: 27, b: 78 },                // #2D1B4E
            fg: Color::Rgb { r: 220, g: 220, b: 220 },             // #dcdcdc
            accent: Color::Rgb { r: 0, g: 255, b: 255 },           // #00ffff cyan
            accent_secondary: Color::Rgb { r: 255, g: 215, b: 0 },   // #ffd700 gold
            accent_tertiary: Color::Rgb { r: 255, g: 77, b: 158 },   // #ff4d9e pink
            muted: Color::Rgb { r: 140, g: 120, b: 160 },          // #8c78a0 muted purple-gray
            border: Color::Rgb { r: 138, g: 43, b: 226 },          // #8a2be2 electric purple
            border_focused: Color::Rgb { r: 0, g: 255, b: 255 },    // #00ffff cyan
            error: Color::Rgb { r: 255, g: 80, b: 80 },             // #ff5050
            success: Color::Rgb { r: 80, g: 255, b: 120 },          // #50ff78
            warning: Color::Rgb { r: 255, g: 200, b: 60 },         // #ffc83c
            selection: Color::Rgb { r: 60, g: 30, b: 80 },          // #3c1e50
            highlight: Color::Rgb { r: 255, g: 77, b: 158 },        // #ff4d9e pink
            shark: Color::Rgb { r: 255, g: 77, b: 158 },             // #ff4d9e pink shark
        }
    }

    pub fn dark_ocean() -> Self {
        Self {
            bg: Color::Rgb { r: 10, g: 15, b: 30 },
            fg: Color::Rgb { r: 200, g: 200, b: 210 },
            accent: Color::Rgb { r: 0, g: 200, b: 255 },
            accent_secondary: Color::Rgb { r: 255, g: 180, b: 60 },
            accent_tertiary: Color::Rgb { r: 200, g: 50, b: 255 },
            muted: Color::Rgb { r: 60, g: 70, b: 90 },
            border: Color::Rgb { r: 80, g: 100, b: 120 },
            border_focused: Color::Rgb { r: 0, g: 200, b: 255 },
            error: Color::Rgb { r: 255, g: 80, b: 80 },
            success: Color::Rgb { r: 80, g: 255, b: 120 },
            warning: Color::Rgb { r: 255, g: 200, b: 60 },
            selection: Color::Rgb { r: 30, g: 40, b: 60 },
            highlight: Color::Rgb { r: 0, g: 200, b: 255 },
            shark: Color::Rgb { r: 255, g: 180, b: 60 },
        }
    }

    pub fn high_contrast() -> Self {
        Self {
            bg: Color::Black,
            fg: Color::White,
            accent: Color::Cyan,
            accent_secondary: Color::Yellow,
            accent_tertiary: Color::Magenta,
            muted: Color::Grey,
            border: Color::White,
            border_focused: Color::Yellow,
            error: Color::Red,
            success: Color::Green,
            warning: Color::Yellow,
            selection: Color::DarkGrey,
            highlight: Color::Yellow,
            shark: Color::Yellow,
        }
    }
}

// Global theme instance (set at startup)
static mut CURRENT_THEME: Theme = Theme::neon_purple();

pub fn set_theme(theme: Theme) {
    unsafe { CURRENT_THEME = theme; }
}

pub fn current_theme() -> Theme {
    unsafe { CURRENT_THEME }
}

// ── ANSI color helpers ──────────────────────────────────────────────────────

/// Convert a crossterm Color to an ANSI foreground escape sequence.
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
        Color::Grey => "\x1b[37m".to_string(),
        _ => "\x1b[0m".to_string(),
    }
}

/// Convert a crossterm Color to an ANSI background escape sequence.
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
        Color::Grey => "\x1b[47m".to_string(),
        _ => "\x1b[0m".to_string(),
    }
}

pub fn ansi_reset() -> &'static str {
    "\x1b[0m"
}

/// Colorize a string with foreground color.
pub fn colorize(text: &str, color: Color) -> String {
    format!("{}{}{}", ansi_fg(color), text, ansi_reset())
}

/// Colorize with bold.
pub fn bold(text: &str, color: Color) -> String {
    format!("\x1b[1m{}{}{}", ansi_fg(color), text, ansi_reset())
}

/// Colorize with italic.
pub fn italic(text: &str, color: Color) -> String {
    format!("\x1b[3m{}{}{}", ansi_fg(color), text, ansi_reset())
}

/// Colorize with bold + italic.
pub fn bold_italic(text: &str, color: Color) -> String {
    format!("\x1b[1;3m{}{}{}", ansi_fg(color), text, ansi_reset())
}

/// Underlined text with color.
pub fn underlined(text: &str, color: Color) -> String {
    format!("\x1b[4m{}{}{}", ansi_fg(color), text, ansi_reset())
}

// ── Convenience colorizers using current theme ───────────────────────────────

pub fn text_color(text: &str) -> String {
    colorize(text, current_theme().fg)
}

pub fn accent_color(text: &str) -> String {
    colorize(text, current_theme().accent)
}

pub fn accent_secondary_color(text: &str) -> String {
    colorize(text, current_theme().accent_secondary)
}

pub fn accent_tertiary_color(text: &str) -> String {
    colorize(text, current_theme().accent_tertiary)
}

pub fn muted_color(text: &str) -> String {
    colorize(text, current_theme().muted)
}

pub fn border_color(text: &str) -> String {
    colorize(text, current_theme().border)
}

pub fn focused_border_color(text: &str) -> String {
    colorize(text, current_theme().border_focused)
}

pub fn error_color(text: &str) -> String {
    colorize(text, current_theme().error)
}

pub fn success_color(text: &str) -> String {
    colorize(text, current_theme().success)
}

pub fn warning_color(text: &str) -> String {
    colorize(text, current_theme().warning)
}

pub fn selection_color(text: &str) -> String {
    colorize(text, current_theme().selection)
}

pub fn highlight_color(text: &str) -> String {
    bold(text, current_theme().highlight)
}

pub fn shark_color(text: &str) -> String {
    bold(text, current_theme().shark)
}

pub fn title_color(text: &str) -> String {
    bold(text, current_theme().accent_secondary)
}

pub fn tool_color(text: &str) -> String {
    colorize(text, current_theme().accent)
}

pub fn wordmark_color(text: &str) -> String {
    bold(text, current_theme().accent_secondary)
}

pub fn prompt_color(text: &str) -> String {
    italic(text, current_theme().muted)
}

pub fn user_msg_color(text: &str) -> String {
    colorize(text, current_theme().accent_tertiary)
}

pub fn assistant_msg_color(text: &str) -> String {
    colorize(text, current_theme().fg)
}

pub fn system_msg_color(text: &str) -> String {
    colorize(text, current_theme().muted)
}

/// Set background color for a region (using ANSI bg + fg reset after).
pub fn bg_colored(text: &str, bg: Color, fg: Color) -> String {
    format!("{}{}{}{}", ansi_bg(bg), ansi_fg(fg), text, ansi_reset())
}
