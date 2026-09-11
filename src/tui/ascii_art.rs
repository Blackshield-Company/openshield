/// OpenShield ASCII Art & Banner Generation
///
/// Blackshield Mercenary styling for the TUI launch screen: bone text, steel
/// borders, and the blood-red shield sigil. Pixel glyphs use solid `█` blocks
/// so the shield and wordmark read as continuous metal, not scattered dots.
use crate::tui::theme::{Color, ansi_fg, ansi_reset};

// ── Blackshield Palette ─────────────────────────────────────────────────────

const C_IRON: Color = Color::Rgb {
    r: 31,
    g: 31,
    b: 40,
}; // #1F1F28 — charcoal, visible against a black terminal
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
const C_BLOOD_DARK: Color = Color::Rgb {
    r: 92,
    g: 10,
    b: 16,
}; // #5C0A10 — dried-blood base of the wordmark
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
const C_SHADOW: Color = Color::Rgb {
    r: 20,
    g: 20,
    b: 26,
}; // #14141A — wordmark drop shadow

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
//
// Gothic stencil wordmark: 5x7 block glyphs composed with 1-cell tracking.
// A dark copy offset one cell left + one cell down yields the drop shadow.
// Face rows: bone-bright top, blood crossbar, steel-ash base.

const GLYPH_ROWS: usize = 7;

fn glyph(ch: char) -> [&'static str; GLYPH_ROWS] {
    match ch {
        'O' => [
            "█████", "█   █", "█   █", "█   █", "█   █", "█   █", "█████",
        ],
        'P' => [
            "█████", "█   █", "█   █", "█████", "█    ", "█    ", "█    ",
        ],
        'E' => [
            "█████", "█    ", "█    ", "████ ", "█    ", "█    ", "█████",
        ],
        'N' => [
            "█   █", "██  █", "██  █", "█ █ █", "█  ██", "█  ██", "█   █",
        ],
        'S' => [
            "█████", "█    ", "█    ", "█████", "    █", "    █", "█████",
        ],
        'H' => [
            "█   █", "█   █", "█   █", "█████", "█   █", "█   █", "█   █",
        ],
        'I' => [
            "█████", "  █  ", "  █  ", "  █  ", "  █  ", "  █  ", "█████",
        ],
        'L' => [
            "█    ", "█    ", "█    ", "█    ", "█    ", "█    ", "█████",
        ],
        'D' => [
            "████ ", "█   █", "█   █", "█   █", "█   █", "█   █", "████ ",
        ],
        _ => ["     "; GLYPH_ROWS],
    }
}

fn face_color(row: usize) -> Color {
    match row {
        0 => C_BLOOD_BRIGHT,     // top edge highlight, like light on carved stone
        1..=5 => C_BLOOD,        // deep blood face
        _ => C_BLOOD_DARK,       // dried-blood base
    }
}

fn openshield_logo() -> Vec<String> {
    let text = "OPENSHIELD";
    let mut face: Vec<Vec<bool>> = vec![Vec::new(); GLYPH_ROWS];
    for (i, ch) in text.chars().enumerate() {
        for (y, row) in glyph(ch).iter().enumerate() {
            if i > 0 {
                face[y].push(false);
            }
            face[y].extend(row.chars().map(|c| c == '█'));
        }
    }
    let w = face[0].len();
    let is_face = |x: usize, y: usize| y < GLYPH_ROWS && x < w && face[y][x];

    (0..=GLYPH_ROWS)
        .map(|oy| {
            let mut out = String::new();
            let mut active: Option<Color> = None;
            for ox in 0..=w {
                let cell = if ox >= 1 && is_face(ox - 1, oy) {
                    Some(face_color(oy))
                } else if oy >= 1 && is_face(ox, oy - 1) {
                    Some(C_SHADOW)
                } else {
                    None
                };
                if cell != active {
                    match cell {
                        Some(c) => out.push_str(&ansi_fg(c)),
                        None => out.push_str(ansi_reset()),
                    }
                    active = cell;
                }
                out.push(if cell.is_some() { '█' } else { ' ' });
            }
            out.push_str(ansi_reset());
            out
        })
        .collect()
}

// ── Blackshield Sigil ───────────────────────────────────────────────────────
//
// Template legend: `#` steel outline · `.` iron field · `X` blood cross ·
// `o` rivet (bone-bright stud on the rim) · space = transparent.
// Every row is exactly 27 cells wide so all rows share one center axis —
// the sigil cannot drift.

const SHIELD_ROWS: &[&str] = &[
    "#o#######################o#",
    "#.........................#",
    "#.........................#",
    "#........XXXXXXXXX........#",
    "#........XXXXXXXXX........#",
    "#...XXX....XXXXX....XXX...#",
    "#...XXXXX..XXXXX..XXXXX...#",
    "o...XXXXXXXXXXXXXXXXXXX...o",
    "#...XXXXX..XXXXX..XXXXX...#",
    " #..XXX....XXXXX....XXX..# ",
    "  #......XXXXXXXXX......#  ",
    "   #.....XXXXXXXXX.....#   ",
    "    #.................#    ",
    "     #...............#     ",
    "      #.............#      ",
    "       #...........#       ",
    "        #.........#        ",
    "         #.......#         ",
    "          #.....#          ",
    "           #...#           ",
    "            ###            ",
];

fn compact_shield() -> Vec<String> {
    SHIELD_ROWS.iter().map(|row| render_shield_row(row)).collect()
}

fn render_shield_row(row: &str) -> String {
    let mut out = String::new();
    let mut active: Option<char> = None;
    for ch in row.chars() {
        let class = match ch {
            '#' | '.' | 'X' | 'o' => Some(ch),
            _ => None,
        };
        if class != active {
            match class {
                Some('#') => out.push_str(&ansi_fg(C_ASH)),
                Some('.') => out.push_str(&ansi_fg(C_IRON)),
                Some('X') => out.push_str(&ansi_fg(C_BLOOD)),
                Some('o') => out.push_str(&ansi_fg(C_BONE_BRIGHT)),
                _ => out.push_str(ansi_reset()),
            }
            active = class;
        }
        out.push(match class {
            Some('o') => '●',
            Some(_) => '█',
            None => ' ',
        });
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
            &crate::utils::truncate_str(session, 42),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shield_rows_share_one_center_axis() {
        for row in compact_shield() {
            assert_eq!(visible_line_width(&row), 27, "row drifted: {row:?}");
        }
    }

    #[test]
    fn banner_renders_sigil_and_prompt() {
        let info = SplashInfo {
            model: "k3".into(),
            provider: "test".into(),
            permissions: "n/a".into(),
            branch: "n/a".into(),
            directory: "/home/synth".into(),
            session: "test-session".into(),
        };
        let out = banner(100, &info);
        assert!(out.contains("Press any key to start"));
        assert!(out.contains('█'));
    }
}
