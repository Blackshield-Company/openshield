/// Direct ANSI rendering for the splash screen.
/// Displays the OpenShark banner with shark ASCII art, version info, and session details.
/// Styled after the Hermes Agent TUI launch screen.
use std::io::{self, stdout, Write};

use crossterm::{
    cursor::MoveTo,
    style::{Print, ResetColor},
    terminal::{Clear, ClearType},
    queue,
};

use crate::tui::ascii_art;
use crate::tui::theme::{ansi_fg, ansi_reset, Color};
use crate::tui::App;

/// Draw the full-screen splash screen using direct ANSI output.
/// Dismissed by any keypress.
pub fn draw_splash_screen(_app: &App, term_width: u16, term_height: u16) -> io::Result<()> {
    let mut out = stdout();

    // Clear screen with background color
    queue!(out, Clear(ClearType::All))?;

    // Get banner text
    let banner_text = ascii_art::banner(term_width as usize);
    let banner_lines: Vec<&str> = banner_text.lines().collect();
    let banner_height = banner_lines.len() as u16;

    // Calculate vertical positioning - center the banner
    let vertical_offset = (term_height.saturating_sub(banner_height + 8)) / 2;

    // Draw banner lines
    for (i, line) in banner_lines.iter().enumerate() {
        let row = vertical_offset + i as u16;
        if row < term_height {
            queue!(out, MoveTo(0, row), Print(line), ResetColor)?;
        }
    }

    let mut current_row = vertical_offset + banner_height + 1;

    // Version info
    let version_text = ascii_art::version_line(
        env!("CARGO_PKG_VERSION"),
        "2026.6.16",
        "c9523d0",
    );
    let version_x = (term_width.saturating_sub(visible_width(&version_text) as u16)) / 2;
    queue!(out, MoveTo(version_x, current_row), Print(&version_text), ResetColor)?;
    current_row += 2;

    // Session info (replaced by system info panel in new banner)
    // Skip session line — it's now part of the two-column panel in ascii_art
    current_row += 2;

    // Welcome message
    let welcome = ascii_art::welcome_message();
    let welcome_x = (term_width.saturating_sub(visible_width(&welcome) as u16)) / 2;
    queue!(out, MoveTo(welcome_x, current_row), Print(&welcome), ResetColor)?;
    current_row += 1;

    // Tip message
    let tip = ascii_art::tip_message();
    let tip_x = (term_width.saturating_sub(visible_width(&tip) as u16)) / 2;
    queue!(out, MoveTo(tip_x, current_row), Print(&tip), ResetColor)?;

    // "Press any key" prompt
    let prompt = format!(
        "{}Press any key to start{}",
        ansi_fg(Color::Rgb { r: 255, g: 77, b: 158 }),
        ansi_reset()
    );
    let prompt_x = (term_width.saturating_sub(visible_width(&prompt) as u16)) / 2;
    queue!(
        out,
        MoveTo(prompt_x, term_height.saturating_sub(3)),
        Print(&prompt),
        ResetColor
    )?;

    out.flush()
}

/// Calculate visible width of a string (ignoring ANSI escape codes).
fn visible_width(s: &str) -> usize {
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
