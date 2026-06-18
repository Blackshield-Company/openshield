/// Direct ANSI rendering for the splash screen.
/// Displays the OpenShark banner with shark ASCII art, version info, and session details.
/// Styled after the Hermes Agent TUI launch screen.
///
/// ANTI-FLICKER: Uses a static flag to only clear+draw once. Subsequent calls
/// just re-flush the existing buffer. This eliminates the seizure-inducing
/// full-screen clear on every 60fps tick.
use std::io::{self, stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::{
    cursor::{Hide, MoveTo},
    style::{Print, ResetColor},
    terminal::{Clear, ClearType},
    queue,
};

use crate::tui::ascii_art;
use crate::tui::theme::{ansi_fg, ansi_reset, Color};
use crate::tui::App;

static SPLASH_DRAWN: AtomicBool = AtomicBool::new(false);

/// Draw the full-screen splash screen using direct ANSI output.
/// Dismissed by any keypress.
///
/// Anti-flicker: only clears and redraws on the first call. After that,
/// the screen stays static until the user dismisses it.
pub fn draw_splash_screen(_app: &App, term_width: u16, term_height: u16) -> io::Result<()> {
    let mut out = stdout();

    // Only clear and draw once — after that, the screen is static.
    // This prevents the seizure-inducing full-screen flicker on every 60fps frame.
    if !SPLASH_DRAWN.load(Ordering::Relaxed) {
        // Hide cursor for clean splash
        queue!(out, Hide)?;

        // Clear screen once
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

        let current_row = vertical_offset + banner_height + 1;

        // Version info
        let version_text = ascii_art::version_line(
            env!("CARGO_PKG_VERSION"),
            "2026.6.16",
            "c9523d0",
        );
        let version_x = (term_width.saturating_sub(visible_width(&version_text) as u16)) / 2;
        queue!(out, MoveTo(version_x, current_row), Print(&version_text), ResetColor)?;

        // Session info (replaced by system info panel in new banner)
        // Skip session line — it's now part of the two-column panel in ascii_art

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

        out.flush()?;

        // Mark as drawn so we never clear again
        SPLASH_DRAWN.store(true, Ordering::Relaxed);
    }

    Ok(())
}

/// Reset the splash drawn flag so it can be drawn again (e.g., on restart).
#[allow(dead_code)]
pub fn reset_splash() {
    SPLASH_DRAWN.store(false, Ordering::Relaxed);
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
