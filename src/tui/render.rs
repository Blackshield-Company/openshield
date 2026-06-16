/// Main UI renderer — direct crossterm ANSI output.
/// Replaces ratatui's Frame/Widget system with raw terminal drawing.
use std::io::{self, stdout, Write};

use crossterm::{
    cursor::{MoveTo, Show},
    style::{Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType, size},
    queue,
};

use crossterm::style::Color;

use crate::tui::components::{chat, sidebar, splash};
use crate::tui::theme::*;
use crate::tui::{App, AppMode};

/// Draw the entire UI based on current app state.
/// This is the main entry point called every frame.
pub fn draw_ui(app: &mut App) -> io::Result<()> {
    let mut out = stdout();
    let (term_width, term_height) = size()?;

    if app.mode == AppMode::Splash {
        return splash::draw_splash_screen(app, term_width, term_height);
    }

    // Layout: sidebar (20%) + chat (80%)
    let sidebar_width = if app.sidebar_expanded {
        (term_width as f32 * 0.2) as u16
    } else {
        0
    };
    let chat_width = term_width - sidebar_width;

    // Input bar height
    let input_height = if app.multi_model_mode { 4 } else { 3 };
    let chat_height = term_height - input_height;

    // Draw sidebar
    if app.sidebar_expanded && sidebar_width > 0 {
        let sidebar_area = (0, 0, sidebar_width, term_height);
        sidebar::draw_sidebar(app, sidebar_area)?;
    }

    // Draw chat area
    let chat_area = (sidebar_width, 0, chat_width, chat_height);
    chat::draw_chat_area(app, chat_area)?;

    // Draw input bar
    let input_area = (sidebar_width, chat_height, chat_width, input_height);
    chat::draw_input_bar(app, input_area)?;

    // Draw popups/overlays on top
    if app.mode == AppMode::ToolApproval {
        draw_tool_approval_popup(app, term_width, term_height)?;
    }

    if app.mode == AppMode::DiffPreview {
        draw_diff_preview_popup(app, term_width, term_height)?;
    }

    if app.show_comparison {
        draw_comparison_overlay(app, term_width, term_height)?;
    }

    // Command palette overlay
    if app.command_palette.visible {
        draw_command_palette_overlay(app, term_width, term_height)?;
    }

    // Bookmark manager overlay
    if app.bookmark_manager.visible {
        draw_bookmark_overlay(app, term_width, term_height)?;
    }

    // Position cursor in input area
    let cursor_x = sidebar_width + 2 + app.cursor_position as u16;
    let cursor_y = chat_height + 1;
    queue!(out, MoveTo(cursor_x.min(term_width - 1), cursor_y), Show)?;

    out.flush()
}

fn draw_tool_approval_popup(_app: &App, term_width: u16, term_height: u16) -> io::Result<()> {
    let mut out = stdout();
    let popup_width = 60u16.min(term_width - 4);
    let popup_height = 20u16.min(term_height - 4);
    let popup_x = (term_width - popup_width) / 2;
    let popup_y = (term_height - popup_height) / 2;

    draw_popup_frame(&mut out, popup_x, popup_y, popup_width, popup_height, " Tool Approval ", current_theme().border_focused)?;
    out.flush()
}

fn draw_diff_preview_popup(_app: &App, term_width: u16, term_height: u16) -> io::Result<()> {
    let mut out = stdout();
    let popup_width = 80u16.min(term_width - 4);
    let popup_height = 30u16.min(term_height - 4);
    let popup_x = (term_width - popup_width) / 2;
    let popup_y = (term_height - popup_height) / 2;

    draw_popup_frame(&mut out, popup_x, popup_y, popup_width, popup_height, " Diff Preview ", current_theme().border_focused)?;
    out.flush()
}

fn draw_comparison_overlay(_app: &App, term_width: u16, term_height: u16) -> io::Result<()> {
    let mut out = stdout();
    let overlay_width = 100u16.min(term_width - 4);
    let overlay_height = 40u16.min(term_height - 4);
    let overlay_x = (term_width - overlay_width) / 2;
    let overlay_y = (term_height - overlay_height) / 2;

    draw_popup_frame(&mut out, overlay_x, overlay_y, overlay_width, overlay_height, " Model Comparison ", current_theme().border_focused)?;
    out.flush()
}

fn draw_command_palette_overlay(app: &App, term_width: u16, term_height: u16) -> io::Result<()> {
    let mut out = stdout();
    let popup_width = (term_width as f32 * 0.6) as u16;
    let popup_height = 20u16.min(term_height - 4);
    let popup_x = (term_width - popup_width) / 2;
    let popup_y = (term_height - popup_height) / 3;

    draw_popup_frame(&mut out, popup_x, popup_y, popup_width, popup_height, " Command Palette ", current_theme().border)?;

    // Filter line
    let filter_y = popup_y + 2;
    let filter_text = if app.command_palette.filter.is_empty() {
        italic("Type to filter commands...", current_theme().muted)
    } else {
        text_color(&app.command_palette.filter)
    };
    queue!(out, MoveTo(popup_x + 2, filter_y), Print(&filter_text), ResetColor, Clear(ClearType::UntilNewLine))?;

    // Commands list
    let filtered = app.command_palette.filtered();
    let list_start_y = popup_y + 4;
    let max_items = (popup_height - 6) as usize;

    for (i, cmd) in filtered.iter().take(max_items).enumerate() {
        let row = list_start_y + i as u16;
        let is_selected = i == app.command_palette.selected;

        let name = if is_selected {
            bg_colored(&format!(" {:12} ", cmd.name), current_theme().selection, current_theme().accent)
        } else {
            format!("{}{:12}{}", ansi_fg(current_theme().accent), cmd.name, ansi_reset())
        };

        let desc = if is_selected {
            bg_colored(&cmd.description, current_theme().selection, current_theme().fg)
        } else {
            format!("{}{}{}", ansi_fg(current_theme().fg), cmd.description, ansi_reset())
        };

        let line = format!("{} {} {}", name, desc,
            cmd.shortcut.as_ref().map(|s| format!("{}[{}]{}", ansi_fg(current_theme().accent_tertiary), s, ansi_reset())).unwrap_or_default()
        );

        queue!(out, MoveTo(popup_x + 2, row), Print(&line), ResetColor, Clear(ClearType::UntilNewLine))?;
    }

    out.flush()
}

fn draw_bookmark_overlay(app: &App, term_width: u16, term_height: u16) -> io::Result<()> {
    let mut out = stdout();
    let popup_width = (term_width as f32 * 0.6) as u16;
    let popup_height = 20u16.min(term_height - 4);
    let popup_x = (term_width - popup_width) / 2;
    let popup_y = (term_height - popup_height) / 3;

    let title = match app.bookmark_manager.mode {
        crate::tui::bookmarks::BookmarkMode::List => " Bookmarks ",
        crate::tui::bookmarks::BookmarkMode::Create => " New Bookmark ",
        crate::tui::bookmarks::BookmarkMode::Confirm => " Confirm ",
    };

    draw_popup_frame(&mut out, popup_x, popup_y, popup_width, popup_height, title, current_theme().border)?;

    match app.bookmark_manager.mode {
        crate::tui::bookmarks::BookmarkMode::List => {
            // Filter line
            let filter_y = popup_y + 2;
            let filter_text = if app.bookmark_manager.filter.is_empty() {
                italic("Type to filter bookmarks...", current_theme().muted)
            } else {
                text_color(&app.bookmark_manager.filter)
            };
            queue!(out, MoveTo(popup_x + 2, filter_y), Print(&filter_text), ResetColor, Clear(ClearType::UntilNewLine))?;

            // Bookmark list
            let filtered = app.bookmark_manager.filtered();
            let list_start_y = popup_y + 4;
            let max_items = (popup_height - 6) as usize;

            for (i, bm) in filtered.iter().take(max_items).enumerate() {
                let row = list_start_y + i as u16;
                let is_selected = i == app.bookmark_manager.selected;

                let name = if is_selected {
                    bg_colored(&format!(" {:20} ", bm.name), current_theme().selection, current_theme().accent)
                } else {
                    format!("{}{:20}{}", ansi_fg(current_theme().accent), bm.name, ansi_reset())
                };

                let desc = if is_selected {
                    bg_colored(&bm.description, current_theme().selection, current_theme().fg)
                } else {
                    format!("{}{}{}", ansi_fg(current_theme().fg), bm.description, ansi_reset())
                };

                let time = if is_selected {
                    bg_colored(&format!("[{}]", bm.created_at), current_theme().selection, current_theme().accent_tertiary)
                } else {
                    format!("{}[{}]{}", ansi_fg(current_theme().accent_tertiary), bm.created_at, ansi_reset())
                };

                let line = format!("{} {} {}", name, desc, time);
                queue!(out, MoveTo(popup_x + 2, row), Print(&line), ResetColor, Clear(ClearType::UntilNewLine))?;
            }
        }
        _ => {}
    }

    out.flush()
}

/// Draw a popup frame with border and title.
fn draw_popup_frame(
    out: &mut impl Write,
    x: u16, y: u16,
    width: u16, height: u16,
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

    queue!(out, MoveTo(x, y), SetForegroundColor(border_color), Print(&top))?;

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
