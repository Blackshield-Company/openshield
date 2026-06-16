/// OpenShark ASCII Art & Banner Generation
///
/// Generates the retro pixel-art style banner for the TUI launch screen.
/// Colors match the original openshark_title_final.png:
/// - Background: dark purple #2D1B4E
/// - Shark fin: pink #E85A9C with highlight #FFB6D9
/// - "OpenShark" text: gradient lavender #D4A5E8 to deep purple #7B2CBF
/// - Tagline: hot pink #FF4D9E
/// - Waves: blue layers #1565C0, #2196F3, #4DD0E1 with white crests
///
/// REFACTORED: Now styled after Hermes TUI / Claw-Code TUI layout:
/// - Large blocky logo header (like CLAW Code)
/// - Two-column system info panel (like both)
/// - Connection status line
/// - Help bar with key commands
/// - Input prompt with blinking cursor

use crate::tui::theme::{ansi_fg, ansi_reset, Color};

// ── Color Constants (OpenShark True Colors) ───────────────────────────────────

const _C_BG: Color = Color::Rgb { r: 45, g: 27, b: 78 }; // #2D1B4E
const C_SHARK_PINK: Color = Color::Rgb { r: 232, g: 90, b: 156 }; // #E85A9C
const C_SHARK_HIGHLIGHT: Color = Color::Rgb { r: 255, g: 182, b: 217 }; // #FFB6D9
const C_LAVENDER_TOP: Color = Color::Rgb { r: 212, g: 165, b: 232 }; // #D4A5E8
const C_LAVENDER_BOT: Color = Color::Rgb { r: 123, g: 44, b: 191 }; // #7B2CBF
const C_TAGLINE: Color = Color::Rgb { r: 255, g: 77, b: 158 }; // #FF4D9E
const C_GOLD: Color = Color::Rgb { r: 255, g: 215, b: 0 }; // #FFD700
const C_CYAN: Color = Color::Rgb { r: 0, g: 255, b: 255 }; // #00FFFF
const C_WAVE_DARK: Color = Color::Rgb { r: 21, g: 101, b: 192 }; // #1565C0
const C_WAVE_MID: Color = Color::Rgb { r: 33, g: 150, b: 243 }; // #2196F3
const C_WAVE_LIGHT: Color = Color::Rgb { r: 77, g: 208, b: 225 }; // #4DD0E1
const C_WHITE: Color = Color::Rgb { r: 255, g: 255, b: 255 };
const C_MUTED: Color = Color::Rgb { r: 140, g: 120, b: 160 }; // muted purple-gray
const C_FG: Color = Color::Rgb { r: 220, g: 220, b: 220 }; // #DCDCDC

// ── Main Banner (Hermes/Claw-Code Style) ────────────────────────────────────

/// The full OpenShark splash screen in Hermes/Claw-Code layout style.
/// Returns a string with embedded ANSI color codes.
pub fn banner(term_width: usize) -> String {
    let mut lines = Vec::new();

    // Top padding
    lines.push(String::new());
    lines.push(String::new());

    // Large blocky "SHARK" logo (like CLAW Code's header)
    let logo_lines = shark_logo();
    for line in logo_lines {
        lines.push(center_line(line, term_width));
    }

    // "Code" with lobster emoji (like CLAW Code's "Code 🦀")
    lines.push(center_line(
        format!("{}  Code 🦞{}", ansi_fg(C_SHARK_PINK), ansi_reset()),
        term_width,
    ));

    lines.push(String::new());

    // Version info (Hermes style: small centered text)
    lines.push(center_line(
        version_line(env!("CARGO_PKG_VERSION"), "2026.6.16", "c9523d0"),
        term_width,
    ));

    lines.push(String::new());

    // Two-column system info panel (like Claw-Code)
    let info_lines = system_info_panel(
        "kimi-k2.7-code",
        "danger-full-access",
        "main",
        "/home/synth",
        "session-1781637801812-0",
    );
    for line in info_lines {
        lines.push(center_line(line, term_width));
    }

    lines.push(String::new());

    // Connection status line
    lines.push(center_line(
        connection_status("kimi-k2.7-code", "openai"),
        term_width,
    ));

    lines.push(String::new());

    // Help bar (like both TUIs)
    lines.push(center_line(help_bar(), term_width));

    lines.push(String::new());

    // Welcome + tip
    lines.push(center_line(welcome_message(), term_width));
    lines.push(center_line(tip_message(), term_width));

    lines.push(String::new());

    // Shark fin art (smaller, below everything)
    let shark_lines = compact_shark();
    for line in shark_lines {
        lines.push(center_line(line, term_width));
    }

    // Input prompt at bottom (like CLAW Code's `>` prompt)
    lines.push(String::new());
    lines.push(center_line(
        format!(
            "{}>{} {}Press any key to start{}",
            ansi_fg(C_SHARK_PINK),
            ansi_reset(),
            ansi_fg(C_TAGLINE),
            ansi_reset()
        ),
        term_width,
    ));

    lines.join("\n")
}

// ── Logo: "SHARK" in blocky pixel style ─────────────────────────────────────

fn shark_logo() -> Vec<String> {
    // Blocky pixel-art "SHARK" — red/pink like CLAW Code's logo
    vec![
        format!(
            "{}███████ ██   ██  █████  ███████ ██   ██{}",
            ansi_fg(C_SHARK_PINK),
            ansi_reset()
        ),
        format!(
            "{}██      ██   ██ ██   ██ ██      ██  ██ {}",
            ansi_fg(C_SHARK_PINK),
            ansi_reset()
        ),
        format!(
            "{}███████ ███████ ███████ █████   █████  {}",
            ansi_fg(C_SHARK_HIGHLIGHT),
            ansi_reset()
        ),
        format!(
            "{}     ██ ██   ██ ██   ██ ██      ██  ██ {}",
            ansi_fg(C_SHARK_PINK),
            ansi_reset()
        ),
        format!(
            "{}███████ ██   ██ ██   ██ ███████ ██   ██{}",
            ansi_fg(C_SHARK_PINK),
            ansi_reset()
        ),
    ]
}

// ── Compact Shark Fin (below the info) ──────────────────────────────────────

fn compact_shark() -> Vec<String> {
    vec![
        format!(
            "                    {}{}▲{}                          ",
            ansi_fg(C_SHARK_PINK),
            ansi_fg(C_SHARK_HIGHLIGHT),
            ansi_reset()
        ),
        format!(
            "                   {}{}/█\\{}                         ",
            ansi_fg(C_SHARK_PINK),
            ansi_fg(C_SHARK_HIGHLIGHT),
            ansi_reset()
        ),
        format!(
            "                  {}{}/███\\{}                        ",
            ansi_fg(C_SHARK_PINK),
            ansi_fg(C_SHARK_HIGHLIGHT),
            ansi_reset()
        ),
        format!(
            "                 {}{}/█████\\{}                       ",
            ansi_fg(C_SHARK_PINK),
            ansi_fg(C_SHARK_HIGHLIGHT),
            ansi_reset()
        ),
        format!(
            "                {}{}/███████\\{}                      ",
            ansi_fg(C_SHARK_PINK),
            ansi_fg(C_SHARK_HIGHLIGHT),
            ansi_reset()
        ),
        format!(
            "               {}{}/█████████\\{}                     ",
            ansi_fg(C_SHARK_PINK),
            ansi_fg(C_SHARK_HIGHLIGHT),
            ansi_reset()
        ),
        format!(
            "{}{}▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓{}{}",
            ansi_fg(C_WAVE_DARK),
            ansi_fg(C_WAVE_MID),
            ansi_fg(C_WAVE_LIGHT),
            ansi_reset()
        ),
        format!(
            "{}{}▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒{}{}",
            ansi_fg(C_WAVE_MID),
            ansi_fg(C_WAVE_LIGHT),
            ansi_fg(C_WHITE),
            ansi_reset()
        ),
        format!(
            "{}░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░{}",
            ansi_fg(C_WAVE_LIGHT),
            ansi_reset()
        ),
    ]
}

// ── System Info Panel (Two-Column, Claw-Code Style) ─────────────────────────

fn system_info_panel(
    model: &str,
    permissions: &str,
    branch: &str,
    directory: &str,
    session: &str,
) -> Vec<String> {
    let label_color = ansi_fg(C_MUTED);
    let value_color = ansi_fg(C_FG);
    let reset = ansi_reset();

    vec![
        format!(
            "{}┌──────────────────────┬────────────────────────────────────────────┐{}",
            ansi_fg(C_GOLD),
            reset
        ),
        format!(
            "{}│{} {:<20} {}│{} {:<42} {}│",
            ansi_fg(C_GOLD),
            label_color,
            "Model",
            reset,
            value_color,
            model,
            reset
        ),
        format!(
            "{}│{} {:<20} {}│{} {:<42} {}│",
            ansi_fg(C_GOLD),
            label_color,
            "Permissions",
            reset,
            value_color,
            permissions,
            reset
        ),
        format!(
            "{}│{} {:<20} {}│{} {:<42} {}│",
            ansi_fg(C_GOLD),
            label_color,
            "Branch",
            reset,
            value_color,
            branch,
            reset
        ),
        format!(
            "{}│{} {:<20} {}│{} {:<42} {}│",
            ansi_fg(C_GOLD),
            label_color,
            "Directory",
            reset,
            value_color,
            directory,
            reset
        ),
        format!(
            "{}│{} {:<20} {}│{} {:<42} {}│",
            ansi_fg(C_GOLD),
            label_color,
            "Session",
            reset,
            value_color,
            &session[..session.len().min(42)],
            reset
        ),
        format!(
            "{}└──────────────────────┴────────────────────────────────────────────┘{}",
            ansi_fg(C_GOLD),
            reset
        ),
    ]
}

// ── Connection Status ─────────────────────────────────────────────────────

fn connection_status(model: &str, provider: &str) -> String {
    format!(
        "{}Connected:{} {} via {}{}",
        ansi_fg(C_MUTED),
        ansi_reset(),
        ansi_fg(C_CYAN) + model + ansi_reset(),
        ansi_fg(C_MUTED) + provider + ansi_reset(),
        ansi_reset()
    )
}

// ── Help Bar ────────────────────────────────────────────────────────────────

pub fn help_bar() -> String {
    format!(
        "{}Type /help for commands · /status for live context · /resume latest · /diff then /commit to ship · Tab for completions · Shift+Enter for newline{}",
        ansi_fg(C_MUTED),
        ansi_reset()
    )
}

// ── Version Line (Hermes Style) ─────────────────────────────────────────────

pub fn version_line(version: &str, date: &str, commit: &str) -> String {
    format!(
        "{}OpenShark v{} ({} · upstream {}){}",
        ansi_fg(C_GOLD),
        version,
        date,
        commit,
        ansi_reset()
    )
}

// ── Welcome & Tip ───────────────────────────────────────────────────────────

pub fn welcome_message() -> String {
    format!(
        "{}Welcome to OpenShark! Type your message or /help for commands.{}",
        ansi_fg(C_FG),
        ansi_reset()
    )
}

pub fn tip_message() -> String {
    format!(
        "{}• Tip: /sethome marks a chat as the home channel for cron job deliveries.{}",
        ansi_fg(C_MUTED),
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

// ── Legacy: Full "OPENSHARK" Title (kept for reference, unused in new layout) ─

#[allow(dead_code)]
fn openshark_title() -> Vec<String> {
    let top_color = C_LAVENDER_TOP;
    let bottom_color = C_LAVENDER_BOT;
    let outline = C_WHITE;

    vec![
        format!(
            "{}{}  ██████  ██████  ███████ ██   ██ ███████  █████  ██████  ██   ██ {}{}",
            ansi_fg(outline),
            ansi_fg(top_color),
            ansi_reset(),
            ansi_reset()
        ),
        format!(
            "{}{}  ██   ██ ██   ██ ██      ██   ██ ██      ██   ██ ██   ██ ██  ██  {}{}",
            ansi_fg(outline),
            ansi_fg(top_color),
            ansi_reset(),
            ansi_reset()
        ),
        format!(
            "{}{}  ██████  ██   ██ █████   ███████ ███████ ███████ ██████  █████   {}{}",
            ansi_fg(outline),
            ansi_fg(bottom_color),
            ansi_reset(),
            ansi_reset()
        ),
        format!(
            "{}{}  ██      ██   ██ ██      ██   ██      ██ ██   ██ ██   ██ ██  ██  {}{}",
            ansi_fg(outline),
            ansi_fg(bottom_color),
            ansi_reset(),
            ansi_reset()
        ),
        format!(
            "{}{}  ██      ██████  ███████ ██   ██ ███████ ██   ██ ██   ██ ██   ██ {}{}",
            ansi_fg(outline),
            ansi_fg(bottom_color),
            ansi_reset(),
            ansi_reset()
        ),
    ]
}

#[allow(dead_code)]
fn shark_with_waves() -> Vec<String> {
    compact_shark()
}
