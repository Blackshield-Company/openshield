/// OpenShield ASCII Art & Banner Generation
///
/// Blackshield Mercenary styling for the TUI launch screen: bone text, steel
/// borders, and the blood-red shield sigil. Pixel glyphs intentionally use
/// `▪` instead of `█` so adjacent cells keep readable gaps in terminal fonts.
use crate::tui::theme::{Color, ansi_fg, ansi_reset};

// ── Blackshield Palette ─────────────────────────────────────────────────────

const C_IRON: Color = Color::Rgb {
    r: 16,
    g: 16,
    b: 20,
}; // #101014
const C_STEEL: Color = Color::Rgb {
    r: 22,
    g: 22,
    b: 28,
}; // #16161C
const C_BONE: Color = Color::Rgb {
    r: 216,
    g: 211,
    b: 200,
}; // #D8D3C8
const C_BONE_BRIGHT: Color = Color::Rgb {
    r: 245,
    g: 241,
    b: 232,
}; // #F5F1E8
const C_BLOOD: Color = Color::Rgb {
    r: 193,
    g: 18,
    b: 31,
}; // #C1121F
const C_BLOOD_BRIGHT: Color = Color::Rgb {
    r: 255,
    g: 107,
    b: 114,
}; // #FF6B72
const C_ASH: Color = Color::Rgb {
    r: 138,
    g: 143,
    b: 152,
}; // #8A8F98
const C_STEEL_BLUE: Color = Color::Rgb {
    r: 123,
    g: 157,
    b: 196,
}; // #7B9DC4
const C_WAR_GOLD: Color = Color::Rgb {
    r: 201,
    g: 162,
    b: 39,
}; // #C9A227
const C_FIELD_GREEN: Color = Color::Rgb {
    r: 106,
    g: 153,
    b: 78,
}; // #6A994E

// ── Main Banner ─────────────────────────────────────────────────────────────

/// Live session info displayed on the splash banner.
/// Populated from the running `App` state so the banner never goes stale.
pub struct SplashInfo {
    pub model: String,
    pub provider: String,
    pub permissions: String,
    pub branch: String,
    pub directory: String,
    pub session: String,
}

/// The full OpenShield splash screen.
/// Returns a string with embedded ANSI color codes.
pub fn banner(term_width: usize, info: &SplashInfo) -> String {
    let mut lines = Vec::new();

    lines.push(String::new());
    lines.push(String::new());

    for line in openshield_logo() {
        lines.push(center_line(line, term_width));
    }

    lines.push(String::new());
    lines.push(center_line(
        version_line(
            env!("CARGO_PKG_VERSION"),
            env!("OS_BUILD_DATE"),
            env!("OS_GIT_HASH"),
        ),
        term_width,
    ));
    lines.push(String::new());

    for line in compact_shield() {
        lines.push(center_line(line, term_width));
    }

    lines.push(String::new());

    for line in system_info_panel(
        &info.model,
        &info.permissions,
        &info.branch,
        &info.directory,
        &info.session,
    ) {
        lines.push(center_line(line, term_width));
    }

    lines.push(String::new());
    lines.push(center_line(
        connection_status(&info.model, &info.provider),
        term_width,
    ));
    lines.push(String::new());
    lines.push(center_line(help_bar(), term_width));
    lines.push(String::new());
    lines.push(center_line(welcome_message(), term_width));
    lines.push(center_line(tip_message(), term_width));
    lines.push(String::new());
    lines.push(center_line(
        format!(
            "{}>{} {}Press any key to start{}",
            ansi_fg(C_BLOOD),
            ansi_reset(),
            ansi_fg(C_BONE),
            ansi_reset()
        ),
        term_width,
    ));

    lines.join("\n")
}

// ── Wordmark ────────────────────────────────────────────────────────────────

fn openshield_logo() -> Vec<String> {
    let rows = [
        " ▪▪▪   ▪▪▪▪  ▪▪▪▪▪ ▪   ▪  ▪▪▪▪ ▪   ▪ ▪▪▪▪▪ ▪▪▪▪▪ ▪     ▪▪▪▪ ",
        "▪   ▪  ▪   ▪ ▪     ▪▪  ▪ ▪     ▪   ▪   ▪   ▪     ▪     ▪   ▪",
        "▪   ▪  ▪▪▪▪  ▪▪▪▪  ▪ ▪ ▪  ▪▪▪  ▪▪▪▪▪   ▪   ▪▪▪▪  ▪     ▪   ▪",
        "▪   ▪  ▪     ▪     ▪  ▪▪     ▪ ▪   ▪   ▪   ▪     ▪     ▪   ▪",
        " ▪▪▪   ▪     ▪▪▪▪▪ ▪   ▪ ▪▪▪▪  ▪   ▪ ▪▪▪▪▪ ▪▪▪▪▪ ▪▪▪▪▪ ▪▪▪▪ ",
    ];

    let colors = [C_BONE_BRIGHT, C_BONE, C_BLOOD_BRIGHT, C_BLOOD, C_BLOOD];

    rows.iter()
        .zip(colors)
        .map(|(row, color)| format!("{}{}{}", ansi_fg(color), row, ansi_reset()))
        .collect()
}

// ── Blackshield Sigil ───────────────────────────────────────────────────────

fn compact_shield() -> Vec<String> {
    vec![
        shield_line(8, &[(9, C_ASH)]),
        shield_line(6, &[(2, C_ASH), (13, C_STEEL), (2, C_ASH)]),
        shield_line(5, &[(2, C_ASH), (15, C_STEEL), (2, C_ASH)]),
        shield_line(
            4,
            &[
                (2, C_ASH),
                (4, C_IRON),
                (3, C_BLOOD),
                (4, C_IRON),
                (2, C_ASH),
            ],
        ),
        shield_line(
            3,
            &[
                (2, C_ASH),
                (4, C_IRON),
                (7, C_BLOOD),
                (4, C_IRON),
                (2, C_ASH),
            ],
        ),
        shield_line(
            3,
            &[
                (2, C_ASH),
                (2, C_IRON),
                (11, C_BLOOD),
                (2, C_IRON),
                (2, C_ASH),
            ],
        ),
        shield_line(
            3,
            &[
                (2, C_ASH),
                (2, C_IRON),
                (11, C_BLOOD),
                (2, C_IRON),
                (2, C_ASH),
            ],
        ),
        shield_line(
            4,
            &[
                (2, C_ASH),
                (4, C_IRON),
                (3, C_BLOOD),
                (4, C_IRON),
                (2, C_ASH),
            ],
        ),
        shield_line(
            5,
            &[
                (2, C_ASH),
                (3, C_IRON),
                (3, C_BLOOD),
                (3, C_IRON),
                (2, C_ASH),
            ],
        ),
        shield_line(
            6,
            &[
                (2, C_ASH),
                (2, C_IRON),
                (5, C_BLOOD),
                (2, C_IRON),
                (2, C_ASH),
            ],
        ),
        shield_line(
            7,
            &[
                (2, C_ASH),
                (1, C_IRON),
                (3, C_BLOOD),
                (1, C_IRON),
                (2, C_ASH),
            ],
        ),
        shield_line(8, &[(2, C_ASH), (3, C_BLOOD), (2, C_ASH)]),
        shield_line(9, &[(5, C_ASH)]),
        shield_line(10, &[(3, C_ASH)]),
        shield_line(11, &[(1, C_BLOOD)]),
    ]
}

fn shield_line(padding: usize, segments: &[(usize, Color)]) -> String {
    let mut out = " ".repeat(padding);
    for (count, color) in segments {
        out.push_str(&ansi_fg(*color));
        out.push_str(&"▪".repeat(*count));
    }
    out.push_str(ansi_reset());
    out
}

// ── System Info Panel ───────────────────────────────────────────────────────

fn system_info_panel(
    model: &str,
    permissions: &str,
    branch: &str,
    directory: &str,
    session: &str,
) -> Vec<String> {
    let label_color = ansi_fg(C_ASH);
    let value_color = ansi_fg(C_BONE);
    let border_color = ansi_fg(C_BLOOD);
    let reset = ansi_reset();

    vec![
        format!(
            "{}┌──────────────────────┬────────────────────────────────────────────┐{}",
            border_color, reset
        ),
        info_row(
            &border_color,
            &label_color,
            &value_color,
            reset,
            "Model",
            model,
        ),
        info_row(
            &border_color,
            &label_color,
            &value_color,
            reset,
            "Permissions",
            permissions,
        ),
        info_row(
            &border_color,
            &label_color,
            &value_color,
            reset,
            "Branch",
            branch,
        ),
        info_row(
            &border_color,
            &label_color,
            &value_color,
            reset,
            "Directory",
            directory,
        ),
        info_row(
            &border_color,
            &label_color,
            &value_color,
            reset,
            "Session",
            &session[..session.len().min(42)],
        ),
        format!(
            "{}└──────────────────────┴────────────────────────────────────────────┘{}",
            border_color, reset
        ),
    ]
}

fn info_row(
    border_color: &str,
    label_color: &str,
    value_color: &str,
    reset: &str,
    label: &str,
    value: &str,
) -> String {
    format!(
        "{}│{} {:<20} {}│{} {:<42} {}│",
        border_color, label_color, label, reset, value_color, value, reset
    )
}

// ── Connection Status ───────────────────────────────────────────────────────

fn connection_status(model: &str, provider: &str) -> String {
    format!(
        "{}Connected:{} {} via {}{}",
        ansi_fg(C_ASH),
        ansi_reset(),
        ansi_fg(C_STEEL_BLUE) + model + ansi_reset(),
        ansi_fg(C_ASH) + provider + ansi_reset(),
        ansi_reset()
    )
}

// ── Help / Version / Messages ───────────────────────────────────────────────

pub fn help_bar() -> String {
    format!(
        "{}Type /help for commands · /status for live context · /resume latest · /diff then /commit to ship · Tab for completions · Shift+Enter for newline{}",
        ansi_fg(C_ASH),
        ansi_reset()
    )
}

pub fn version_line(version: &str, date: &str, commit: &str) -> String {
    format!(
        "{}OpenShield v{} ({} · upstream {}){}",
        ansi_fg(C_WAR_GOLD),
        version,
        date,
        commit,
        ansi_reset()
    )
}

pub fn welcome_message() -> String {
    format!(
        "{}Welcome to OpenShield. Type your message or /help for commands.{}",
        ansi_fg(C_BONE),
        ansi_reset()
    )
}

pub fn tip_message() -> String {
    format!(
        "{}• Tip: /sethome marks a chat as the home channel for cron job deliveries.{}",
        ansi_fg(C_FIELD_GREEN),
        ansi_reset()
    )
}

// ── Utilities ───────────────────────────────────────────────────────────────

/// Center a line of text within the given width.
fn center_line(line: String, width: usize) -> String {
    let visible_width = visible_line_width(&line);
    if visible_width >= width {
        return line;
    }
    let padding = (width - visible_width) / 2;
    format!("{}{}", " ".repeat(padding), line)
}

/// Calculate visible width of a string (ignoring ANSI escape codes).
fn visible_line_width(s: &str) -> usize {
    let mut width = 0usize;
    let mut in_escape = false;
    for ch in s.chars() {
        if ch == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            width += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        }
    }
    width
}
