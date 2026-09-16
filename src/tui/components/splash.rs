/// Direct ANSI rendering for the splash screen.
/// Displays the OpenShield banner with Blackshield sigil art, version info, and session details.
/// Styled after the Hermes Agent TUI launch screen.
///
/// ANTI-FLICKER: Uses a static flag to only clear+draw once. Subsequent calls
/// just re-flush the existing buffer. This eliminates the seizure-inducing
/// full-screen clear on every 60fps tick.
use std::io::{self, Write, stdout};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossterm::{
    cursor::{Hide, MoveTo},
    queue,
    style::{Print, ResetColor},
    terminal::{Clear, ClearType},
};

use crate::tui::App;
use crate::tui::ascii_art;
use crate::tui::sprites::Sprite;
use crate::tui::theme::{ansi_fg, ansi_reset, current_theme};

static SPLASH_DRAWN: AtomicBool = AtomicBool::new(false);

/// Sprite animation state, cached from the initial full draw so the
/// per-tick animated redraw does not rebuild the banner (which spawns git).
/// (top_row, height, max_width); height == 0 means no sprite was drawn.
static SPRITE_TOP: AtomicUsize = AtomicUsize::new(0);
static SPRITE_HEIGHT: AtomicUsize = AtomicUsize::new(0);
static SPRITE_MAX_WIDTH: AtomicUsize = AtomicUsize::new(0);
/// Monotonic tick driving the frame cadence; last-drawn frame index.
static SPLASH_TICK: AtomicUsize = AtomicUsize::new(0);
static SPRITE_FRAME: AtomicUsize = AtomicUsize::new(0);

/// Frames advance every N splash ticks (~60fps tick → ~6fps animation).
const TICKS_PER_FRAME: usize = 10;

/// Draw the full-screen splash screen using direct ANSI output.
/// Dismissed by any keypress.
///
/// Anti-flicker: only clears and redraws on the first call. After that,
/// the screen stays static until the user dismisses it — except the mascot
/// sprite region, which is re-drawn in place when the sprite animates.
pub fn draw_splash_screen(app: &App, term_width: u16, term_height: u16) -> io::Result<()> {
    let mut out = stdout();

    if !SPLASH_DRAWN.load(Ordering::Relaxed) {
        // Hide cursor for clean splash
        queue!(out, Hide)?;

        // Clear screen once
        queue!(out, Clear(ClearType::All))?;

        // Build live splash info from App state so the banner never goes stale
        let directory = if app.project_path.is_empty() {
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string())
        } else {
            app.project_path.clone()
        };
        let info = ascii_art::SplashInfo {
            model: app.model.clone(),
            provider: app.provider.name.clone(),
            permissions: app.profile_registry.active().to_string(),
            branch: detect_git_branch(&directory),
            session: app.session_id.clone(),
            directory,
        };

        // Get banner text
        let banner_text = ascii_art::banner(term_width as usize, &info);
        let banner_lines: Vec<&str> = banner_text.lines().collect();
        let banner_height = banner_lines.len() as u16;

        // ── Mascot sprite layout ──────────────────────────────────────────
        // Drawn centered above the banner when there is vertical room;
        // omitted gracefully on short terminals (banner-only splash).
        let sprite = app.sprite.as_ref();
        let sprite_height = sprite
            .map(max_frame_height)
            .unwrap_or(0)
            .min(term_height as usize) as u16;
        // Room for banner + sprite + a blank separator row + the bottom margin.
        let sprite_fits = sprite.is_some()
            && term_height
                >= banner_height
                    .saturating_add(sprite_height)
                    .saturating_add(6);
        let sprite_block = if sprite_fits { sprite_height + 1 } else { 0 };

        // Calculate vertical positioning - center banner (+ sprite) as a group
        let vertical_offset = (term_height.saturating_sub(banner_height + 8 + sprite_block)) / 2;

        // Draw the sprite above the banner
        if sprite_fits && let Some(sprite) = sprite {
            let max_width = sprite_max_frame_width(sprite).min(term_width as usize);
            let top = vertical_offset;
            draw_sprite_frame(&mut out, sprite, 0, top, term_width, max_width)?;
            SPRITE_TOP.store(top as usize, Ordering::Relaxed);
            SPRITE_HEIGHT.store(sprite_height as usize, Ordering::Relaxed);
            SPRITE_MAX_WIDTH.store(max_width, Ordering::Relaxed);
            SPRITE_FRAME.store(0, Ordering::Relaxed);
        }

        // Draw banner lines below the sprite
        let banner_top = vertical_offset + sprite_block;
        for (i, line) in banner_lines.iter().enumerate() {
            let row = banner_top + i as u16;
            if row < term_height {
                queue!(out, MoveTo(0, row), Print(line), ResetColor)?;
            }
        }

        // "Press any key" prompt
        let prompt = format!(
            "{}Press any key to start{}",
            ansi_fg(current_theme().accent),
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
    } else if app.sprite.as_ref().is_some_and(|s| s.frame_count() > 1)
        && SPRITE_HEIGHT.load(Ordering::Relaxed) > 0
    {
        // Animated redraw: only the sprite region, in place, on frame change.
        // Same-width frames overwrite cleanly (rows are padded to the frame
        // max width), so no full-screen clear is needed.
        let tick = SPLASH_TICK.fetch_add(1, Ordering::Relaxed);
        let sprite = app.sprite.as_ref().expect("checked above");
        let frame_idx = (tick / TICKS_PER_FRAME) % sprite.frame_count();
        if frame_idx != SPRITE_FRAME.load(Ordering::Relaxed) {
            let top = SPRITE_TOP.load(Ordering::Relaxed) as u16;
            let max_width = SPRITE_MAX_WIDTH.load(Ordering::Relaxed);
            draw_sprite_frame(&mut out, sprite, frame_idx, top, term_width, max_width)?;
            SPRITE_FRAME.store(frame_idx, Ordering::Relaxed);
            out.flush()?;
        }
    }

    Ok(())
}

/// Draw one sprite frame centered at `top`, padding rows to `max_width`
/// so a previous (longer) frame never leaves residue behind.
fn draw_sprite_frame(
    out: &mut impl Write,
    sprite: &Sprite,
    frame_idx: usize,
    top: u16,
    term_width: u16,
    max_width: usize,
) -> io::Result<()> {
    let frame = &sprite.frames[frame_idx % sprite.frame_count()];
    for (i, line) in frame.lines().enumerate() {
        let visible = visible_width(line);
        let padding = max_width.saturating_sub(visible);
        let x = (term_width as usize).saturating_sub(max_width) / 2;
        queue!(
            out,
            MoveTo(x as u16, top + i as u16),
            Print(line),
            Print(" ".repeat(padding)),
            ResetColor
        )?;
    }
    Ok(())
}

/// Height of the tallest frame (all frames occupy the same row band).
fn max_frame_height(sprite: &Sprite) -> usize {
    sprite
        .frames
        .iter()
        .map(|f| f.lines().count())
        .max()
        .unwrap_or(0)
}

/// Widest visible line across all frames — the frame band's clear width.
fn sprite_max_frame_width(sprite: &Sprite) -> usize {
    sprite
        .frames
        .iter()
        .flat_map(|f| f.lines())
        .map(visible_width)
        .max()
        .unwrap_or(0)
}

/// Reset the splash drawn flag so it can be drawn again (e.g., on restart).
#[allow(dead_code)]
pub fn reset_splash() {
    SPLASH_DRAWN.store(false, Ordering::Relaxed);
    SPRITE_TOP.store(0, Ordering::Relaxed);
    SPRITE_HEIGHT.store(0, Ordering::Relaxed);
    SPRITE_MAX_WIDTH.store(0, Ordering::Relaxed);
    SPLASH_TICK.store(0, Ordering::Relaxed);
    SPRITE_FRAME.store(0, Ordering::Relaxed);
}

/// Detect the current git branch in the given directory.
/// Returns "n/a" when not in a repo or git is unavailable.
fn detect_git_branch(dir: &str) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "n/a".to_string())
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
