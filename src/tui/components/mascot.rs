//! Persistent corner mascot — the active sprite's `-mini` variant stands at
//! the bottom-right of the chat area at ALL times (petdex-style), animated
//! on the TUI tick. Drawn last on every full redraw so it survives repaints;
//! between redraws, `tick_redraw` repaints just the mascot region in place
//! when the animation frame advances.
use std::io::{self, Write, stdout};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Print, ResetColor},
};

use super::splash::{max_frame_height, sprite_max_frame_width, visible_width};
use crate::tui::App;
use crate::tui::sprites::Sprite;

/// Mascot animation cadence: advance one frame per N 60fps ticks (~3fps).
const TICKS_PER_FRAME: usize = 20;

/// Cached geometry from the last full draw so tick repaints don't need
/// the chat layout. `drawn == false` means the mascot is currently hidden.
static MASCOT_X: AtomicUsize = AtomicUsize::new(0);
static MASCOT_TOP: AtomicUsize = AtomicUsize::new(0);
static MASCOT_MAX_WIDTH: AtomicUsize = AtomicUsize::new(0);
static MASCOT_DRAWN: AtomicBool = AtomicBool::new(false);
static LAST_FRAME: AtomicUsize = AtomicUsize::new(usize::MAX);

fn current_frame(app: &App) -> usize {
    app.sprite_mini
        .as_ref()
        .map(|s| (app.spinner_frame / TICKS_PER_FRAME) % s.frame_count())
        .unwrap_or(0)
}

/// Draw the mascot at the bottom-right of the chat area.
/// `chat_bottom` is the exclusive bottom row of the chat area (the input
/// bar starts there). Called at the end of every full UI redraw.
pub fn draw_mascot(
    out: &mut impl Write,
    app: &App,
    term_width: u16,
    chat_bottom: u16,
) -> io::Result<()> {
    let Some(sprite) = app.sprite_mini.as_ref() else {
        MASCOT_DRAWN.store(false, Ordering::Relaxed);
        return Ok(());
    };
    let height = max_frame_height(sprite) as u16;
    let max_width = sprite_max_frame_width(sprite);
    // Hide when the mascot would crowd the UI: narrow terminals need the
    // columns for chat text, short ones the rows.
    if term_width < max_width as u16 + 40 || chat_bottom < height + 4 {
        MASCOT_DRAWN.store(false, Ordering::Relaxed);
        return Ok(());
    }
    let x = term_width - max_width as u16 - 2;
    let top = chat_bottom - height;

    MASCOT_X.store(x as usize, Ordering::Relaxed);
    MASCOT_TOP.store(top as usize, Ordering::Relaxed);
    MASCOT_MAX_WIDTH.store(max_width, Ordering::Relaxed);
    MASCOT_DRAWN.store(true, Ordering::Relaxed);

    let frame = current_frame(app);
    paint(out, sprite, frame, top, x, max_width)?;
    LAST_FRAME.store(frame, Ordering::Relaxed);
    Ok(())
}

/// Called on every TUI tick: repaint the mascot in place when its animation
/// frame advanced since the last paint. No-op while hidden or single-frame.
pub fn tick_redraw(app: &App) {
    if !MASCOT_DRAWN.load(Ordering::Relaxed) {
        return;
    }
    let Some(sprite) = app.sprite_mini.as_ref() else {
        return;
    };
    if sprite.frame_count() < 2 {
        return;
    }
    let frame = current_frame(app);
    if frame == LAST_FRAME.load(Ordering::Relaxed) {
        return;
    }
    let top = MASCOT_TOP.load(Ordering::Relaxed) as u16;
    let x = MASCOT_X.load(Ordering::Relaxed) as u16;
    let max_width = MASCOT_MAX_WIDTH.load(Ordering::Relaxed);
    let mut out = stdout();
    if paint(&mut out, sprite, frame, top, x, max_width).is_ok() {
        let _ = out.flush();
        LAST_FRAME.store(frame, Ordering::Relaxed);
    }
}

/// Paint one frame at (x, top), padding every row to `max_width` so the
/// previous frame leaves no residue.
fn paint(
    out: &mut impl Write,
    sprite: &Sprite,
    frame_idx: usize,
    top: u16,
    x: u16,
    max_width: usize,
) -> io::Result<()> {
    let frame = &sprite.frames[frame_idx % sprite.frame_count()];
    for (i, line) in frame.lines().enumerate() {
        let padding = max_width.saturating_sub(visible_width(line));
        queue!(
            out,
            MoveTo(x, top + i as u16),
            Print(line),
            Print(" ".repeat(padding)),
            ResetColor
        )?;
    }
    Ok(())
}
