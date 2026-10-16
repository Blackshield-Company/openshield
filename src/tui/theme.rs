#![allow(dead_code)]

/// Blackshield theme system for OpenShield.
///
/// Blood, steel, bone, and void — the same palette as the Blackshield
/// Mercenary desktop theme. This module uses raw ANSI color codes (via
/// crossterm) instead of ratatui's Style system.
pub use crossterm::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
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

impl Default for Theme {
    fn default() -> Self {
        Self::blackshield()
    }
}

impl Theme {
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "blackshield" | "default" => Some(Self::blackshield()),
            "steel_blue" => Some(Self::steel_blue()),
            "high_contrast" => Some(Self::high_contrast()),
            _ => None,
        }
    }

    pub fn names() -> Vec<&'static str> {
        vec!["blackshield", "steel_blue", "high_contrast"]
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn border_unfocused(&self) -> Color {
        self.border
    }

    /// Black Steel palette (docs/brand/tokens.css).
    pub const fn blackshield() -> Self {
        Self {
            name: "blackshield",
            bg: Color::Rgb {
                r: 14,
                g: 14,
                b: 18,
            }, // void #0E0E12
            fg: Color::Rgb {
                r: 228,
                g: 222,
                b: 210,
            }, // bone #E4DED2
            accent: Color::Rgb {
                r: 193,
                g: 18,
                b: 31,
            }, // blood #C1121F
            accent_secondary: Color::Rgb {
                r: 212,
                g: 167,
                b: 44,
            }, // ember #D4A72C
            accent_tertiary: Color::Rgb {
                r: 154,
                g: 160,
                b: 168,
            }, // steel #9AA0A8
            muted: Color::Rgb {
                r: 154,
                g: 160,
                b: 168,
            }, // steel #9AA0A8
            border: Color::Rgb {
                r: 52,
                g: 52,
                b: 63,
            }, // edge #34343F
            border_focused: Color::Rgb {
                r: 193,
                g: 18,
                b: 31,
            }, // blood
            error: Color::Rgb {
                r: 224,
                g: 48,
                b: 60,
            }, // blood-hi #E0303C
            success: Color::Rgb {
                r: 94,
                g: 158,
                b: 90,
            }, // moss #5E9E5A
            warning: Color::Rgb {
                r: 212,
                g: 167,
                b: 44,
            }, // ember
            selection: Color::Rgb {
                r: 193,
                g: 18,
                b: 31,
            }, // blood
            highlight: Color::Rgb {
                r: 224,
                g: 48,
                b: 60,
            }, // blood-hi #E0303C
            shield: Color::Rgb {
                r: 193,
                g: 18,
                b: 31,
            }, // blood shield
        }
    }

    pub const fn steel_blue() -> Self {
        Self {
            name: "steel_blue",
            bg: Color::Rgb {
                r: 13,
                g: 13,
                b: 17,
            },
            fg: Color::Rgb {
                r: 216,
                g: 211,
                b: 200,
            },
            accent: Color::Rgb {
                r: 123,
                g: 157,
                b: 196,
            }, // steel-blue-bright #7B9DC4
            accent_secondary: Color::Rgb {
                r: 201,
                g: 162,
                b: 39,
            },
            accent_tertiary: Color::Rgb {
                r: 164,
                g: 80,
                b: 139,
            }, // royal-purple #A4508B
            muted: Color::Rgb {
                r: 138,
                g: 143,
                b: 152,
            },
            border: Color::Rgb {
                r: 26,
                g: 26,
                b: 32,
            },
            border_focused: Color::Rgb {
                r: 123,
                g: 157,
                b: 196,
            },
            error: Color::Rgb {
                r: 193,
                g: 18,
                b: 31,
            },
            success: Color::Rgb {
                r: 106,
                g: 153,
                b: 78,
            },
            warning: Color::Rgb {
                r: 201,
                g: 162,
                b: 39,
            },
            selection: Color::Rgb {
                r: 22,
                g: 22,
                b: 28,
            }, // steel #16161C
            highlight: Color::Rgb {
                r: 123,
                g: 157,
                b: 196,
            },
            shield: Color::Rgb {
                r: 193,
                g: 18,
                b: 31,
            },
        }
    }

    pub const fn high_contrast() -> Self {
        Self {
            name: "high_contrast",
            bg: Color::Black,
            fg: Color::White,
            accent: Color::Red,
            accent_secondary: Color::Yellow,
            accent_tertiary: Color::Cyan,
            muted: Color::Grey,
            border: Color::White,
            border_focused: Color::Red,
            error: Color::Red,
            success: Color::Green,
            warning: Color::Yellow,
            selection: Color::DarkGrey,
            highlight: Color::White,
            shield: Color::Red,
        }
    }
}

// Global theme instance (set at startup). Blackshield is the default.
static mut CURRENT_THEME: Theme = Theme::blackshield();

pub fn set_theme(theme: Theme) {
    unsafe {
        CURRENT_THEME = theme;
    }
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

pub fn shield_color(text: &str) -> String {
    bold(text, current_theme().shield)
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
