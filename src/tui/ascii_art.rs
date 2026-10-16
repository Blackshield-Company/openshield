/// OpenShield ASCII Art & Banner Generation
///
/// Black Steel styling for the TUI launch screen: bone text, steel borders,
/// and the flat shield with a plain blood-red Greek cross. Pixel glyphs use solid `█` blocks
/// so the shield and wordmark read as continuous metal, not scattered dots.
use crate::tui::theme::{Color, ansi_fg, ansi_reset};

// ── Black Steel Palette (docs/brand/tokens.css) ─────────────────────────────

const C_IRON: Color = Color::Rgb {
    r: 34,
    g: 34,
    b: 42,
}; // #22222A plate — shield field, visible on a black terminal
const C_BONE: Color = Color::Rgb {
    r: 228,
    g: 222,
    b: 210,
}; // #E4DED2 bone
const C_BLOOD: Color = Color::Rgb {
    r: 193,
    g: 18,
    b: 31,
}; // #C1121F blood
const C_STEEL: Color = Color::Rgb {
    r: 154,
    g: 160,
    b: 168,
}; // #9AA0A8 steel
const C_MOSS: Color = Color::Rgb {
    r: 94,
    g: 158,
    b: 90,
}; // #5E9E5A moss
const C_EDGE: Color = Color::Rgb {
    r: 52,
    g: 52,
    b: 63,
}; // #34343F edge
const C_SHADOW: Color = Color::Rgb {
    r: 34,
    g: 34,
    b: 42,
}; // #22222A — wordmark drop shadow

// ── Main Banner ─────────────────────────────────────────────────────────────

/// Live session info displayed on the splash banner.
/// Populated from the running `App` state so the banner never goes stale.
#[derive(Clone)]
pub struct SplashInfo {
    pub model: String,
    pub provider: String,
    pub permissions: String,
    pub branch: String,
    pub directory: String,
    pub session: String,
    /// Draw the shield sigil beside the wordmark. Off when the mascot
    /// sprite is shown, so the splash carries one shield, not two.
    pub show_shield: bool,
}

/// The full OpenShield splash screen.
/// Returns a string with embedded ANSI color codes.
pub fn banner(term_width: usize, info: &SplashInfo) -> String {
    // Right-hand column: wordmark, version, session panel, connection.
    let mut right: Vec<String> = Vec::new();
    right.extend(openshield_logo());
    right.push(String::new());
    right.push(version_line(
        env!("CARGO_PKG_VERSION"),
        env!("OS_BUILD_DATE"),
        env!("OS_GIT_HASH"),
    ));
    right.push(String::new());
    right.extend(system_info_panel(
        &info.model,
        &info.permissions,
        &info.branch,
        &info.directory,
        &info.session,
    ));
    right.push(String::new());
    right.push(connection_status(&info.model, &info.provider));

    let mut lines = vec![String::new()];

    // One block: shield on the left, details on the right. Falls back to
    // the details alone when the shield is off or the terminal is narrow.
    let shield = compact_shield();
    let right_width = right
        .iter()
        .map(|l| visible_line_width(l))
        .max()
        .unwrap_or(0);
    let block_width = SHIELD_WIDTH + SHIELD_GAP + right_width;
    if info.show_shield && term_width >= block_width + 2 {
        let rows = shield.len().max(right.len());
        let right_top = shield.len().saturating_sub(right.len()) / 2;
        for i in 0..rows {
            let left = shield
                .get(i)
                .cloned()
                .unwrap_or_else(|| " ".repeat(SHIELD_WIDTH));
            let r = i
                .checked_sub(right_top)
                .and_then(|j| right.get(j))
                .cloned()
                .unwrap_or_default();
            let row = format!("{left}{}{r}", " ".repeat(SHIELD_GAP));
            lines.push(pad_block_line(row, block_width, term_width));
        }
    } else {
        for r in right {
            lines.push(pad_block_line(r, right_width, term_width));
        }
    }

    lines.push(String::new());
    lines.push(center_line(help_bar(), term_width));
    lines.push(String::new());
    lines.push(center_line(welcome_message(), term_width));
    lines.push(center_line(tip_message(), term_width));

    lines.join("\n")
}

/// Width of every `SHIELD_ROWS` row, and the gap between shield and details.
const SHIELD_WIDTH: usize = 27;
const SHIELD_GAP: usize = 4;

/// Left-pad a row so a block of `block_width` columns sits centered.
fn pad_block_line(line: String, block_width: usize, term_width: usize) -> String {
    let padding = term_width.saturating_sub(block_width) / 2;
    format!("{}{}", " ".repeat(padding), line)
}

/// The live prompt drawn at the bottom of the splash: the first keystroke
/// goes straight into the input line.
pub fn splash_prompt() -> String {
    format!(
        "{}❯{} {}▌{}  {}start typing, or /help{}",
        ansi_fg(C_BLOOD),
        ansi_reset(),
        ansi_fg(C_BLOOD),
        ansi_reset(),
        ansi_fg(C_STEEL),
        ansi_reset()
    )
}

// ── Wordmark ────────────────────────────────────────────────────────────────
//
// Gothic stencil wordmark: 5x7 block glyphs composed with 1-cell tracking.
// A dark copy offset one cell left + one cell down yields the drop shadow.
// Face rows: bone face, steel base.

const GLYPH_ROWS: usize = 7;

fn glyph(ch: char) -> [&'static str; GLYPH_ROWS] {
    match ch {
        'O' => [
            "█████",
            "█   █",
            "█   █",
            "█   █",
            "█   █",
            "█   █",
            "█████",
        ],
        'P' => [
            "█████",
            "█   █",
            "█   █",
            "█████",
            "█    ",
            "█    ",
            "█    ",
        ],
        'E' => [
            "█████",
            "█    ",
            "█    ",
            "████ ",
            "█    ",
            "█    ",
            "█████",
        ],
        'N' => [
            "█   █",
            "██  █",
            "██  █",
            "█ █ █",
            "█  ██",
            "█  ██",
            "█   █",
        ],
        'S' => [
            "█████",
            "█    ",
            "█    ",
            "█████",
            "    █",
            "    █",
            "█████",
        ],
        'H' => [
            "█   █",
            "█   █",
            "█   █",
            "█████",
            "█   █",
            "█   █",
            "█   █",
        ],
        'I' => [
            "█████",
            "  █  ",
            "  █  ",
            "  █  ",
            "  █  ",
            "  █  ",
            "█████",
        ],
        'L' => [
            "█    ",
            "█    ",
            "█    ",
            "█    ",
            "█    ",
            "█    ",
            "█████",
        ],
        'D' => [
            "████ ",
            "█   █",
            "█   █",
            "█   █",
            "█   █",
            "█   █",
            "████ ",
        ],
        _ => ["     "; GLYPH_ROWS],
    }
}

fn face_color(row: usize) -> Color {
    match row {
        0..=5 => C_BONE, // bone face
        _ => C_STEEL,    // steel base
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
// Template legend: `#` steel rim · `.` iron field · `X` blood cross (plain
// Greek cross, equal arms in cell aspect) · space = transparent.
// Every row is exactly 27 cells wide so all rows share one center axis —
// the sigil cannot drift.

const SHIELD_ROWS: &[&str] = &[
    "###########################",
    "#.........................#",
    "#..........XXXXX..........#",
    "#..........XXXXX..........#",
    "#..........XXXXX..........#",
    "#...XXXXXXXXXXXXXXXXXXX...#",
    "#...XXXXXXXXXXXXXXXXXXX...#",
    "#...XXXXXXXXXXXXXXXXXXX...#",
    "#..........XXXXX..........#",
    " #.........XXXXX.........# ",
    "  #........XXXXX........#  ",
    "   #.......XXXXX.......#   ",
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
    SHIELD_ROWS
        .iter()
        .map(|row| render_shield_row(row))
        .collect()
}

fn render_shield_row(row: &str) -> String {
    let mut out = String::new();
    let mut active: Option<char> = None;
    for ch in row.chars() {
        let class = match ch {
            '#' | '.' | 'X' => Some(ch),
            _ => None,
        };
        if class != active {
            match class {
                Some('#') => out.push_str(&ansi_fg(C_STEEL)),
                Some('.') => out.push_str(&ansi_fg(C_IRON)),
                Some('X') => out.push_str(&ansi_fg(C_BLOOD)),
                _ => out.push_str(ansi_reset()),
            }
            active = class;
        }
        out.push(if class.is_some() { '█' } else { ' ' });
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
    let label_color = ansi_fg(C_STEEL);
    let value_color = ansi_fg(C_BONE);
    let border_color = ansi_fg(C_EDGE);
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
        "{}●{} {}Connected:{} {} via {}{}",
        ansi_fg(C_MOSS),
        ansi_reset(),
        ansi_fg(C_STEEL),
        ansi_reset(),
        ansi_fg(C_BONE) + model + ansi_reset(),
        ansi_fg(C_STEEL) + provider + ansi_reset(),
        ansi_reset()
    )
}

// ── Help / Version / Messages ───────────────────────────────────────────────

pub fn help_bar() -> String {
    format!(
        "{}Type /help for commands · /status for live context · /resume latest · /diff then /commit to ship · Tab for completions · Shift+Enter for newline{}",
        ansi_fg(C_STEEL),
        ansi_reset()
    )
}

pub fn version_line(version: &str, date: &str, commit: &str) -> String {
    format!(
        "{}OpenShield v{} ({} · upstream {}){}",
        ansi_fg(C_STEEL),
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
        ansi_fg(C_STEEL),
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
    fn shield_is_plain_greek_cross() {
        // Cross rows are symmetric and the cross has no rivets.
        for row in SHIELD_ROWS {
            assert!(!row.contains('o'));
            let rev: String = row.chars().rev().collect();
            assert_eq!(*row, rev, "row not symmetric: {row:?}");
        }
    }

    #[test]
    fn banner_omits_shield_when_sprite_shown() {
        let info = SplashInfo {
            model: "k3".into(),
            provider: "test".into(),
            permissions: "n/a".into(),
            branch: "n/a".into(),
            directory: "/home/synth".into(),
            session: "s".into(),
            show_shield: false,
        };
        let with = banner(
            120,
            &SplashInfo {
                show_shield: true,
                ..info.clone()
            },
        );
        let without = banner(120, &info);
        assert!(with.lines().count() > without.lines().count());
    }

    #[test]
    fn shield_rows_share_one_center_axis() {
        for row in compact_shield() {
            assert_eq!(visible_line_width(&row), 27, "row drifted: {row:?}");
        }
    }

    #[test]
    fn banner_renders_sigil_without_key_gate() {
        let info = SplashInfo {
            model: "k3".into(),
            provider: "test".into(),
            permissions: "n/a".into(),
            branch: "n/a".into(),
            directory: "/home/synth".into(),
            session: "test-session".into(),
            show_shield: true,
        };
        let out = banner(120, &info);
        assert!(!out.contains("Press any key"));
        assert!(out.contains('█'));
        assert!(out.contains("Welcome to OpenShield"));
        assert!(splash_prompt().contains('❯'));
    }
}
