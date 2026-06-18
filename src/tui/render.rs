/// Main UI renderer — direct crossterm ANSI output.
/// Replaces the sidebar+chat layout with a single unified scrollable feed.
/// Everything scrolls through the chat: system info, messages, status.
/// Only a minimal top status bar stays pinned.
///
/// ANTI-FLICKER STRATEGY:
/// - One Clear(ClearType::All) at the start of each normal frame.
/// - No per-line Clear(UntilNewLine) — that causes visible flicker at 60fps.
/// - All drawing is queued, then flushed exactly once at the end.
use std::io::{self, stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    style::{Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType, size},
    queue,
};

use crossterm::style::Color;

use crate::tui::components::{chat, splash};
use crate::tui::theme::*;
use crate::tui::{App, AppMode};

/// Draw the entire UI based on current app state.
/// Single-pane layout: full-width chat feed that scrolls everything.
/// Top status bar is pinned. No sidebar.
/// Tracks whether the screen needs a full redraw.
/// Only set to true when app state changes (input, scroll, new messages, resize).
static NEEDS_REDRAW: AtomicBool = AtomicBool::new(true);

/// Mark the screen as needing a full redraw. Called when state changes.
pub fn request_redraw() {
    NEEDS_REDRAW.store(true, Ordering::Relaxed);
}

pub fn draw_ui(app: &mut App) -> io::Result<()> {
    let mut out = stdout();
    let (term_width, term_height) = size()?;

    if app.mode == AppMode::Splash {
        return splash::draw_splash_screen(app, term_width, term_height);
    }

    // ── Anti-flicker: only clear and redraw when something changed ──────────
    // Most frames are idle (no input, no streaming). Clearing 60x/sec causes
    // visible flicker as the terminal default background flashes between clear
    // and redraw. We only redraw when the app state actually changed.
    if !NEEDS_REDRAW.load(Ordering::Relaxed) {
        return Ok(());
    }
    NEEDS_REDRAW.store(false, Ordering::Relaxed);

    queue!(out, Hide, Clear(ClearType::All))?;

    // ── Layout ──────────────────────────────────────────────────────────────
    // Top status bar: 1 line
    // Chat feed: everything else minus dynamically-sized input bar at bottom
    // Input bar: min 3 lines, max 40% of terminal height, grows with text
    let status_height = 1u16;
    let prompt_width = 2u16; // "> "
    let available_width = term_width.saturating_sub(prompt_width + 2) as usize;
    let needed_input_lines = chat::calculate_input_lines(&app.input, available_width);
    let max_input_height = ((term_height as f32 * 0.4) as u16).clamp(6, 30);
    let input_height = (1 + needed_input_lines as u16).clamp(3, max_input_height);
    let chat_height = term_height.saturating_sub(status_height + input_height);

    let full_width = term_width;

    // ── Draw top status bar (pinned, full width) ──────────────────────────
    draw_top_status_bar(&mut out, app, full_width, status_height)?;

    // ── Draw unified chat feed (full width, scrolls everything) ─────────────
    let chat_area = (0, status_height, full_width, chat_height);
    chat::draw_unified_feed(&mut out, app, chat_area)?;

    // ── Draw input bar (full width, bottom) ─────────────────────────────────
    let input_area = (0, status_height + chat_height, full_width, input_height);
    chat::draw_input_bar(&mut out, app, input_area)?;

    // ── Draw popups/overlays on top ────────────────────────────────────────
    if app.mode == AppMode::ToolApproval {
        draw_tool_approval_popup(&mut out, app, term_width, term_height)?;
    }

    if app.mode == AppMode::DiffPreview {
        draw_diff_preview_popup(&mut out, app, term_width, term_height)?;
    }

    if app.show_comparison {
        draw_comparison_overlay(&mut out, app, term_width, term_height)?;
    }

    // Command palette overlay
    if app.command_palette.visible {
        draw_command_palette_overlay(&mut out, app, term_width, term_height)?;
    }

    // Bookmark manager overlay
    if app.bookmark_manager.visible {
        draw_bookmark_overlay(&mut out, app, term_width, term_height)?;
    }

    // Position cursor in input area — account for text wrapping on narrow terminals
    let (cursor_row, cursor_col) = if available_width == 0 || app.input.is_empty() {
        (0u16, 0u16)
    } else {
        let pre_cursor = &app.input[..app.cursor_position.min(app.input.len())];
        let wrapped = chat::wrap_input_text(pre_cursor, available_width);
        let row = wrapped.len().saturating_sub(1) as u16;
        let col = wrapped.last().map(|s| s.chars().count()).unwrap_or(0) as u16;
        (row, col)
    };
    let cursor_x = 1 + prompt_width + cursor_col;
    let cursor_y = status_height + chat_height + 1 + cursor_row;
    queue!(out, MoveTo(cursor_x.min(term_width - 1), cursor_y), Show)?;

    // Single flush for the entire frame — no mid-frame flushes anywhere
    out.flush()
}

/// Draw a minimal top status bar: model | ctx | progress | status
fn draw_top_status_bar(
    out: &mut impl Write,
    app: &App,
    width: u16,
    _height: u16,
) -> io::Result<()> {
    let gold = Color::Rgb { r: 255, g: 215, b: 0 };
    let cyan = Color::Rgb { r: 0, g: 255, b: 255 };
    let green = Color::Rgb { r: 80, g: 255, b: 120 };
    let muted = Color::Rgb { r: 140, g: 120, b: 160 };

    let model_short = app.model.split('/').next_back().unwrap_or(&app.model);
    let model_part = format!("{}{}{}", ansi_fg(cyan), model_short, ansi_reset());

    let ctx_used = app.context_used();
    let ctx_total = app.model_context_length;
    let ctx_pct = (ctx_used * 100)
        .checked_div(ctx_total)
        .unwrap_or(0) as u16;
    let ctx_part = format!(
        "{}ctx {}%{} ({}/{})",
        ansi_fg(muted),
        ctx_pct,
        ansi_reset(),
        ctx_used,
        ctx_total
    );

    let progress = if app.is_streaming {
        let elapsed = app
            .stream_start_time
            .map(|s| s.elapsed().as_secs())
            .unwrap_or(0);
        let bars = (elapsed % 10) as usize;
        let filled = "█".repeat(bars);
        let empty = "░".repeat(10 - bars);
        let elapsed_str = format_elapsed(app.stream_start_time);
        format!(
            "{}[{}{}]{} {}",
            ansi_fg(green),
            filled,
            empty,
            ansi_reset(),
            elapsed_str
        )
    } else {
        format!("{}[░░░░░░░░░░]{} --", ansi_fg(muted), ansi_reset())
    };

    let status = if app.is_streaming {
        format!("{}[STREAM]{}", ansi_fg(gold), ansi_reset())
    } else {
        format!("{}[OK]{}", ansi_fg(green), ansi_reset())
    };

    let left = format!("{} {} | {}", model_part, ctx_part, progress);
    let right = format!("{} | 🦈 v{}", status, env!("CARGO_PKG_VERSION"));

    let left_width = visible_width(&left);
    let right_width = visible_width(&right);
    let padding = width.saturating_sub(left_width as u16 + right_width as u16);

    let line = format!("{}{}{}", left, " ".repeat(padding as usize), right);

    queue!(out, MoveTo(0, 0), Print(&line), ResetColor)?;
    Ok(())
}

fn format_elapsed(start: Option<std::time::Instant>) -> String {
    match start {
        Some(s) => {
            let secs = s.elapsed().as_secs();
            format!("{}s", secs)
        }
        None => "--".to_string(),
    }
}

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

fn draw_tool_approval_popup(
    out: &mut impl Write,
    _app: &App,
    term_width: u16,
    term_height: u16,
) -> io::Result<()> {
    let popup_width = 60u16.min(term_width - 4);
    let popup_height = 20u16.min(term_height - 4);
    let popup_x = (term_width - popup_width) / 2;
    let popup_y = (term_height - popup_height) / 2;

    draw_popup_frame(
        out,
        popup_x,
        popup_y,
        popup_width,
        popup_height,
        " Tool Approval ",
        current_theme().border_focused,
    )?;
    Ok(())
}

fn draw_diff_preview_popup(
    out: &mut impl Write,
    _app: &App,
    term_width: u16,
    term_height: u16,
) -> io::Result<()> {
    let popup_width = 80u16.min(term_width - 4);
    let popup_height = 30u16.min(term_height - 4);
    let popup_x = (term_width - popup_width) / 2;
    let popup_y = (term_height - popup_height) / 2;

    draw_popup_frame(
        out,
        popup_x,
        popup_y,
        popup_width,
        popup_height,
        " Diff Preview ",
        current_theme().border_focused,
    )?;
    Ok(())
}

fn draw_comparison_overlay(
    out: &mut impl Write,
    _app: &App,
    term_width: u16,
    term_height: u16,
) -> io::Result<()> {
    let overlay_width = 100u16.min(term_width - 4);
    let overlay_height = 40u16.min(term_height - 4);
    let overlay_x = (term_width - overlay_width) / 2;
    let overlay_y = (term_height - overlay_height) / 2;

    draw_popup_frame(
        out,
        overlay_x,
        overlay_y,
        overlay_width,
        overlay_height,
        " Model Comparison ",
        current_theme().border_focused,
    )?;
    Ok(())
}

fn draw_command_palette_overlay(
    out: &mut impl Write,
    app: &App,
    term_width: u16,
    term_height: u16,
) -> io::Result<()> {
    let popup_width = (term_width as f32 * 0.6) as u16;
    let popup_height = 20u16.min(term_height - 4);
    let popup_x = (term_width - popup_width) / 2;
    let popup_y = (term_height - popup_height) / 3;

    draw_popup_frame(
        out,
        popup_x,
        popup_y,
        popup_width,
        popup_height,
        " Command Palette ",
        current_theme().border,
    )?;

    // Filter line
    let filter_y = popup_y + 2;
    let filter_text = if app.command_palette.filter.is_empty() {
        italic("Type to filter commands...", current_theme().muted)
    } else {
        text_color(&app.command_palette.filter)
    };
    queue!(out, MoveTo(popup_x + 2, filter_y), Print(&filter_text), ResetColor)?;

    // Commands list
    let filtered = app.command_palette.filtered();
    let list_start_y = popup_y + 4;
    let max_items = (popup_height - 6) as usize;

    for (i, cmd) in filtered.iter().take(max_items).enumerate() {
        let row = list_start_y + i as u16;
        let is_selected = i == app.command_palette.selected;

        let name = if is_selected {
            bg_colored(
                &format!(" {:12} ", cmd.name),
                current_theme().selection,
                current_theme().accent,
            )
        } else {
            format!(
                "{}{:12}{}",
                ansi_fg(current_theme().accent),
                cmd.name,
                ansi_reset()
            )
        };

        let desc = if is_selected {
            bg_colored(
                &cmd.description,
                current_theme().selection,
                current_theme().fg,
            )
        } else {
            format!(
                "{}{}{}",
                ansi_fg(current_theme().fg),
                cmd.description,
                ansi_reset()
            )
        };

        let line = format!(
            "{} {} {}",
            name,
            desc,
            cmd.shortcut
                .as_ref()
                .map(|s| format!(
                    "{}[{}]{}",
                    ansi_fg(current_theme().accent_tertiary),
                    s,
                    ansi_reset()
                ))
                .unwrap_or_default()
        );

        queue!(out, MoveTo(popup_x + 2, row), Print(&line), ResetColor)?;
    }

    Ok(())
}

fn draw_bookmark_overlay(
    out: &mut impl Write,
    app: &App,
    term_width: u16,
    term_height: u16,
) -> io::Result<()> {
    let popup_width = (term_width as f32 * 0.6) as u16;
    let popup_height = 20u16.min(term_height - 4);
    let popup_x = (term_width - popup_width) / 2;
    let popup_y = (term_height - popup_height) / 3;

    let title = match app.bookmark_manager.mode {
        crate::tui::bookmarks::BookmarkMode::List => " Bookmarks ",
        crate::tui::bookmarks::BookmarkMode::Create => " New Bookmark ",
        crate::tui::bookmarks::BookmarkMode::Confirm => " Confirm ",
    };

    draw_popup_frame(
        out,
        popup_x,
        popup_y,
        popup_width,
        popup_height,
        title,
        current_theme().border,
    )?;

    if app.bookmark_manager.mode == crate::tui::bookmarks::BookmarkMode::List {
        // Filter line
        let filter_y = popup_y + 2;
        let filter_text = if app.bookmark_manager.filter.is_empty() {
            italic("Type to filter bookmarks...", current_theme().muted)
        } else {
            text_color(&app.bookmark_manager.filter)
        };
        queue!(out, MoveTo(popup_x + 2, filter_y), Print(&filter_text), ResetColor)?;

        // Bookmark list
        let filtered = app.bookmark_manager.filtered();
        let list_start_y = popup_y + 4;
        let max_items = (popup_height - 6) as usize;

        for (i, bm) in filtered.iter().take(max_items).enumerate() {
            let row = list_start_y + i as u16;
            let is_selected = i == app.bookmark_manager.selected;

            let name = if is_selected {
                bg_colored(
                    &format!(" {:20} ", bm.name),
                    current_theme().selection,
                    current_theme().accent,
                )
            } else {
                format!(
                    "{}{:20}{}",
                    ansi_fg(current_theme().accent),
                    bm.name,
                    ansi_reset()
                )
            };

            let desc = if is_selected {
                bg_colored(
                    &bm.description,
                    current_theme().selection,
                    current_theme().fg,
                )
            } else {
                format!(
                    "{}{}{}",
                    ansi_fg(current_theme().fg),
                    bm.description,
                    ansi_reset()
                )
            };

            let time = if is_selected {
                bg_colored(
                    &format!("[{}]", bm.created_at),
                    current_theme().selection,
                    current_theme().accent_tertiary,
                )
            } else {
                format!(
                    "{}[{}]{}",
                    ansi_fg(current_theme().accent_tertiary),
                    bm.created_at,
                    ansi_reset()
                )
            };

            let line = format!("{} {} {}", name, desc, time);
            queue!(out, MoveTo(popup_x + 2, row), Print(&line), ResetColor)?;
        }
    }

    Ok(())
}

/// Draw a popup frame with border and title.
fn draw_popup_frame(
    out: &mut impl Write,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    title: &str,
    border_color: Color,
) -> io::Result<()> {
    for row in 0..height {
        queue!(out, MoveTo(x, y + row), Clear(ClearType::UntilNewLine))?;
    }

    // Top border with title
    let title_len = title.len();
    let left = ((width as usize - title_len) / 2).saturating_sub(1);
    let right = (width as usize).saturating_sub(left + title_len + 2);
    let top = format!("┌{} {} {}", "─".repeat(left), title, "─".repeat(right));

    queue!(
        out,
        MoveTo(x, y),
        SetForegroundColor(border_color),
        Print(&top)
    )?;

    // Side borders
    for row in 1..height - 1 {
        queue!(out, MoveTo(x, y + row), Print("│"))?;
        queue!(out, MoveTo(x + width - 1, y + row), Print("│"))?;
    }

    // Bottom border
    let bottom = format!("└{}┘", "─".repeat((width - 2) as usize));
    queue!(out, MoveTo(x, y + height - 1), Print(&bottom), ResetColor)?;

    Ok(())
}
