/// Direct ANSI rendering for the splash screen.
/// Displays the OpenShield banner with the Black Steel shield, version info,
/// session details and a live prompt.
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

static SPLASH_DRAWN: AtomicBool = AtomicBool::new(false);

/// Sprite animation state, cached from the initial full draw so the
/// per-tick animated redraw does not rebuild the banner (which spawns git).
/// (top_row, height, max_width); height == 0 means no sprite was drawn.
static SPRITE_TOP: AtomicUsize = AtomicUsize::new(0);
static SPRITE_HEIGHT: AtomicUsize = AtomicUsize::new(0);
static SPRITE_MAX_WIDTH: AtomicUsize = AtomicUsize::new(0);
static SPRITE_X: AtomicUsize = AtomicUsize::new(0);
/// Monotonic tick driving the frame cadence; last-drawn frame index.
static SPLASH_TICK: AtomicUsize = AtomicUsize::new(0);
static SPRITE_FRAME: AtomicUsize = AtomicUsize::new(0);

/// Frames advance every N splash ticks (~60fps tick → ~6fps animation).
const TICKS_PER_FRAME: usize = 10;

/// Draw the full-screen splash screen using direct ANSI output.
/// Dismissed by the first keypress, which also reaches the input line.
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
            show_shield: app.sprite.is_none(),
        };

        // Get banner text
        let banner_text = ascii_art::banner(term_width as usize, &info);
        let banner_lines: Vec<&str> = banner_text.lines().collect();
        let banner_height = banner_lines.len() as u16;

        // Banner content width = widest trimmed line (padding is invisible).
        // NOTE: wide footer rows (help bar) sit BELOW the sprite's vertical
        // band, so the fit check only measures banner rows inside the band.
        let sprite = app.sprite.as_ref();
        let sprite_height = sprite
            .map(max_frame_height)
            .unwrap_or(0)
            .min(term_height as usize) as u16;
        let sprite_width = sprite.map(sprite_max_frame_width).unwrap_or(0);

        const SPRITE_GAP: u16 = 4;

        // Pass 1: try side-by-side with the banner at its natural offset.
        let mut vertical_offset = (term_height.saturating_sub(banner_height + 8)) / 2;
        let mut banner_top = vertical_offset;
        let sprite_band_top = banner_top + (banner_height.saturating_sub(sprite_height)) / 2;

        // Left edge of banner content within the sprite band only.
        let band_min_start = banner_lines
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                let row = banner_top + *i as u16;
                row >= sprite_band_top
                    && row < sprite_band_top.saturating_add(sprite_height)
                    && !l.trim().is_empty()
            })
            .map(|(_, l)| visible_width(l) - visible_width(l.trim_start()))
            .min()
            .unwrap_or(term_width as usize);

        let side_fits = sprite.is_some()
            && sprite_height <= banner_height
            && band_min_start > sprite_width + SPRITE_GAP as usize;

        // Pass 2: stacked fallback on tall, narrow terminals — the banner
        // group shifts down to make room for the sprite above it.
        let stacked_fits = !side_fits
            && sprite.is_some()
            && term_height
                >= banner_height
                    .saturating_add(sprite_height)
                    .saturating_add(9);
        if stacked_fits {
            vertical_offset =
                (term_height.saturating_sub(banner_height + 8 + sprite_height + 1)) / 2;
            banner_top = vertical_offset + sprite_height + 1;
        }

        // Draw banner first; the sprite overwrites only blank padding.
        for (i, line) in banner_lines.iter().enumerate() {
            let row = banner_top + i as u16;
            if row < term_height {
                queue!(out, MoveTo(0, row), Print(line), ResetColor)?;
            }
        }

        // Draw the sprite: beside the banner, or stacked above it.
        if (side_fits || stacked_fits)
            && let Some(sprite) = sprite
        {
            let max_width = sprite_width.min(term_width as usize);
            let (x, top) = if side_fits {
                let x = (band_min_start - SPRITE_GAP as usize - sprite_width) as u16;
                (x, sprite_band_top)
            } else {
                // Stacked: centered horizontally, directly above the banner.
                let x = (term_width.saturating_sub(max_width as u16)) / 2;
                (x, vertical_offset)
            };
            draw_sprite_frame(
                &mut out,
                sprite.frames_for(app.sprite_state()),
                0,
                top,
                x,
                max_width,
            )?;
            SPRITE_TOP.store(top as usize, Ordering::Relaxed);
            SPRITE_X.store(x as usize, Ordering::Relaxed);
            SPRITE_HEIGHT.store(sprite_height as usize, Ordering::Relaxed);
            SPRITE_MAX_WIDTH.store(max_width, Ordering::Relaxed);
            SPRITE_FRAME.store(0, Ordering::Relaxed);
        }

        // Live prompt: the first keystroke dismisses the splash and lands in
        // the input line (see handle_input), so there is no key gate.
        let prompt = ascii_art::splash_prompt();
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
    } else if app
        .sprite
        .as_ref()
        .is_some_and(|s| s.frames_for(app.sprite_state()).len() > 1)
        && SPRITE_HEIGHT.load(Ordering::Relaxed) > 0
    {
        // Animated redraw: only the sprite region, in place, on frame change.
        // Same-width frames overwrite cleanly (rows are padded to the frame
        // max width), so no full-screen clear is needed.
        let tick = SPLASH_TICK.fetch_add(1, Ordering::Relaxed);
        let sprite = app.sprite.as_ref().expect("checked above");
        let frames = sprite.frames_for(app.sprite_state());
        let frame_idx = (tick / TICKS_PER_FRAME) % frames.len();
        if frame_idx != SPRITE_FRAME.load(Ordering::Relaxed) {
            let top = SPRITE_TOP.load(Ordering::Relaxed) as u16;
            let x = SPRITE_X.load(Ordering::Relaxed) as u16;
            let max_width = SPRITE_MAX_WIDTH.load(Ordering::Relaxed);
            draw_sprite_frame(&mut out, frames, frame_idx, top, x, max_width)?;
            SPRITE_FRAME.store(frame_idx, Ordering::Relaxed);
            out.flush()?;
        }
    }

    Ok(())
}

/// Draw one sprite frame at (`left`, `top`), padding rows to `max_width`
/// so a previous (longer) frame never leaves residue behind.
fn draw_sprite_frame(
    out: &mut impl Write,
    frames: &[String],
    frame_idx: usize,
    top: u16,
    left: u16,
    max_width: usize,
) -> io::Result<()> {
    let Some(frame) = frames.get(frame_idx % frames.len().max(1)) else {
        return Ok(());
    };
    for (i, line) in frame.lines().enumerate() {
        let visible = visible_width(line);
        let padding = max_width.saturating_sub(visible);
        queue!(
            out,
            MoveTo(left, top + i as u16),
            Print(line),
            Print(" ".repeat(padding)),
            ResetColor
        )?;
    }
    Ok(())
}

/// Every frame of every animation state.
fn all_frames(sprite: &Sprite) -> impl Iterator<Item = &String> {
    sprite
        .frames
        .iter()
        .chain(sprite.states.iter().flat_map(|(_, f)| f.iter()))
}

/// Height of the tallest frame (all frames occupy the same row band).
pub(crate) fn max_frame_height(sprite: &Sprite) -> usize {
    all_frames(sprite)
        .map(|f| f.lines().count())
        .max()
        .unwrap_or(0)
}

/// Widest visible line across all frames — the frame band's clear width.
pub(crate) fn sprite_max_frame_width(sprite: &Sprite) -> usize {
    all_frames(sprite)
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
    SPRITE_X.store(0, Ordering::Relaxed);
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
pub(crate) fn visible_width(s: &str) -> usize {
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

    /// Diagnostic + regression: the side-by-side layout decision with the
    /// real banner at a realistic terminal size (200x60). The sprite must
    /// fit to the left of the banner content inside its vertical band.
    #[test]
    fn side_by_side_fits_at_200x60() {
        let info = ascii_art::SplashInfo {
            model: "k3".to_string(),
            provider: "kimi".to_string(),
            permissions: "coding".to_string(),
            branch: "main".to_string(),
            session: "test-session".to_string(),
            directory: "/tmp/test".to_string(),
            show_shield: false,
        };
        let term_width = 200u16;
        let term_height = 60u16;
        let banner_text = ascii_art::banner(term_width as usize, &info);
        let banner_lines: Vec<&str> = banner_text.lines().collect();
        let banner_height = banner_lines.len() as u16;

        let sprite = crate::tui::sprites::load_sprite_or_stock("blackshield-mercenary").0;
        let sprite_height = max_frame_height(&sprite) as u16;
        let sprite_width = sprite_max_frame_width(&sprite);

        const SPRITE_GAP: u16 = 4;
        let vertical_offset = (term_height.saturating_sub(banner_height + 8)) / 2;
        let banner_top = vertical_offset;
        let sprite_band_top = banner_top + (banner_height.saturating_sub(sprite_height)) / 2;

        let band_min_start = banner_lines
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                let row = banner_top + *i as u16;
                row >= sprite_band_top
                    && row < sprite_band_top.saturating_add(sprite_height)
                    && !l.trim().is_empty()
            })
            .map(|(_, l)| visible_width(l) - visible_width(l.trim_start()))
            .min()
            .unwrap_or(term_width as usize);

        eprintln!(
            "banner_height={banner_height} band_min_start={band_min_start} \
             sprite_height={sprite_height} sprite_width={sprite_width}"
        );

        let side_fits =
            sprite_height <= banner_height && band_min_start > sprite_width + SPRITE_GAP as usize;
        assert!(
            side_fits,
            "sprite must fit beside the banner at 200 cols \
             (band_min_start={band_min_start}, need {})",
            sprite_width + SPRITE_GAP as usize + 1
        );

        // No overlap: sprite's right edge stays SPRITE_GAP left of content.
        let sprite_x = band_min_start - SPRITE_GAP as usize - sprite_width;
        assert!(sprite_x + sprite_width + SPRITE_GAP as usize <= band_min_start);
    }

    /// The stock sprite must never be taller than the banner, otherwise the
    /// side-by-side layout is skipped and the sprite stacks above it.
    #[test]
    fn stock_sprite_no_taller_than_banner() {
        let info = ascii_art::SplashInfo {
            model: "k3".to_string(),
            provider: "kimi".to_string(),
            permissions: "coding".to_string(),
            branch: "main".to_string(),
            session: "test-session".to_string(),
            directory: "/tmp/test".to_string(),
            show_shield: false,
        };
        let sprite = crate::tui::sprites::load_stock_sprite();
        let sprite_height = max_frame_height(&sprite);
        for width in [100usize, 120, 160, 200] {
            let banner_height = ascii_art::banner(width, &info).lines().count();
            assert!(
                sprite_height <= banner_height,
                "sprite ({sprite_height} rows) taller than banner ({banner_height} rows) at {width} cols"
            );
        }
    }
}
