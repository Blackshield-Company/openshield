/// Direct ANSI rendering for the sidebar.
/// Refactored to match Hermes/Claw-Code TUI style:
/// - Cleaner info panel with two-column metadata layout
/// - Gold border accents
/// - Compact sections with better visual hierarchy
/// - Status bar at bottom with model/timer/progress
use std::io::{self, stdout, Write};

use crossterm::{
    cursor::MoveTo,
    style::{Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
    queue,
};

use crate::tui::theme::{ansi_fg, ansi_reset, bold, current_theme, italic, Color};
use crate::tui::App;

/// Draw the sidebar with direct ANSI output.
/// `area` is (x, y, width, height) in terminal coordinates.
pub fn draw_sidebar(app: &App, area: (u16, u16, u16, u16)) -> io::Result<()> {
    let (x, y, width, height) = area;
    let mut out = stdout();

    let _border_color = current_theme().border;
    let gold = Color::Rgb { r: 255, g: 215, b: 0 };

    // Outer border with gold accent
    queue!(out, MoveTo(x, y), SetForegroundColor(gold))?;
    queue!(
        out,
        Print("┌"),
        Print("─".repeat((width - 2) as usize)),
        Print("┐")
    )?;
    for row in 1..height - 1 {
        queue!(out, MoveTo(x, y + row), Print("│"))?;
        queue!(out, MoveTo(x + width - 1, y + row), Print("│"))?;
    }
    queue!(
        out,
        MoveTo(x, y + height - 1),
        Print("└"),
        Print("─".repeat((width - 2) as usize)),
        Print("┘")
    )?;
    queue!(out, ResetColor)?;

    let inner_x = x + 1;
    let inner_width = width.saturating_sub(2) as usize;
    let mut row = 1u16;

    // Header: SHARK logo (compact)
    let header = format!(
        "{}🦞 {} {}",
        ansi_fg(Color::Rgb { r: 255, g: 77, b: 158 }),
        bold("OpenShark", gold),
        ansi_fg(Color::Rgb { r: 140, g: 120, b: 160 })
    ) + "v1.1.0" + ansi_reset();
    let header_x = inner_x + ((inner_width - unicode_width::UnicodeWidthStr::width(header.as_str())) / 2) as u16;
    queue!(out, MoveTo(header_x, y + row), Print(&header), ResetColor)?;
    row += 1;

    let tagline = italic("Fast. Precise. Hungry.", Color::Rgb { r: 140, g: 120, b: 160 });
    let tagline_x = inner_x + ((inner_width - unicode_width::UnicodeWidthStr::width(tagline.as_str())) / 2) as u16;
    queue!(out, MoveTo(tagline_x, y + row), Print(&tagline), ResetColor)?;
    row += 2;

    // ── System Info Panel (two-column, Claw-Code style) ──
    draw_panel_title(&mut out, inner_x, y + row, inner_width, " System ", gold)?;
    row += 1;

    let info_lines = vec![
        format_info_line("Model", &app.model, inner_width),
        format_info_line("Session", &app.session_id[..app.session_id.len().min(16)], inner_width),
        format_info_line("Ctx", &format!("{} / {}", app.context_used(), app.model_context_length), inner_width),
        format_info_line("Tokens", &app.tokens_used.to_string(), inner_width),
        format_info_line("Tools", &app.tool_calls_count.to_string(), inner_width),
    ];
    for line in info_lines {
        queue!(out, MoveTo(inner_x, y + row), Print(&line), Clear(ClearType::UntilNewLine), ResetColor)?;
        row += 1;
    }
    row += 1;

    // ── Shortcuts ──
    draw_panel_title(&mut out, inner_x, y + row, inner_width, " Shortcuts ", gold)?;
    row += 1;

    let shortcuts = vec![
        ("Ctrl+Q", "Quit"),
        ("Ctrl+L", "Clear"),
        ("Ctrl+B", "Sidebar"),
        ("Ctrl+M", "Model"),
        ("Ctrl+A", "Auto"),
        ("Ctrl+T", "Theme"),
        ("Ctrl+S", "Tools"),
    ];
    for (key, desc) in shortcuts {
        let line = format!(
            "{}{:8}{} {}",
            ansi_fg(Color::Rgb { r: 0, g: 255, b: 255 }),
            key,
            ansi_fg(Color::Rgb { r: 220, g: 220, b: 220 }),
            desc
        );
        queue!(out, MoveTo(inner_x, y + row), Print(&line), Clear(ClearType::UntilNewLine), ResetColor)?;
        row += 1;
    }
    row += 1;

    // ── Tools/Skills ──
    draw_panel_title(&mut out, inner_x, y + row, inner_width, " Tools ", gold)?;
    row += 1;

    let tools = crate::tools::get_tools();
    for t in tools.iter().take(6) {
        let line = format!(
            "{}{}{} {}",
            ansi_fg(Color::Rgb { r: 0, g: 255, b: 255 }),
            t.name(),
            ansi_fg(Color::Rgb { r: 140, g: 120, b: 160 }),
            t.description()
        );
        queue!(out, MoveTo(inner_x, y + row), Print(&line), Clear(ClearType::UntilNewLine), ResetColor)?;
        row += 1;
    }
    row += 1;

    // ── Performance ──
    draw_panel_title(&mut out, inner_x, y + row, inner_width, " Perf ", gold)?;
    row += 1;

    let perf_lines = vec![
        format_info_line("First", &format!("{}ms", app.session_perf.first_token_ms.last().copied().unwrap_or(0)), inner_width),
        format_info_line("Total", &format!("{}ms", app.session_perf.total_latency_ms.last().copied().unwrap_or(0)), inner_width),
        format_info_line("Tool", &format!("{}ms", app.session_perf.tool_exec_ms.last().copied().unwrap_or(0)), inner_width),
        format_info_line("Reqs", &app.session_perf.requests.to_string(), inner_width),
    ];
    for line in perf_lines {
        queue!(out, MoveTo(inner_x, y + row), Print(&line), Clear(ClearType::UntilNewLine), ResetColor)?;
        row += 1;
    }

    // ── Status Bar (bottom of sidebar, Hermes style) ──
    let status_y = y + height - 2;
    let status_bar = format_status_bar(
        &app.model,
        app.is_streaming,
        app.stream_start_time,
        app.tokens_used,
    );
    queue!(out, MoveTo(inner_x, status_y), Print(&status_bar), Clear(ClearType::UntilNewLine), ResetColor)?;

    // Clear remaining rows between perf and status
    for r in row..status_y {
        queue!(out, MoveTo(inner_x, y + r), Clear(ClearType::UntilNewLine), ResetColor)?;
    }

    out.flush()
}

/// Format a two-column info line: "Label      Value"
fn format_info_line(label: &str, value: &str, width: usize) -> String {
    let label_color = ansi_fg(Color::Rgb { r: 140, g: 120, b: 160 });
    let value_color = ansi_fg(Color::Rgb { r: 220, g: 220, b: 220 });
    let reset = ansi_reset();
    let label_width = 8usize;
    let value_width = width.saturating_sub(label_width + 1);
    format!(
        "{}{:>label_width$}{} {}{:<value_width$}{}",
        label_color,
        label,
        reset,
        value_color,
        value.chars().take(value_width).collect::<String>(),
        reset,
        label_width = label_width,
        value_width = value_width
    )
}

/// Draw a panel section title with horizontal rules.
fn draw_panel_title(
    out: &mut impl Write,
    x: u16,
    y: u16,
    width: usize,
    title: &str,
    color: Color,
) -> io::Result<()> {
    let title_len = unicode_width::UnicodeWidthStr::width(title);
    let left = (width - title_len) / 2;
    let right = width - title_len - left;
    let line = format!(
        "{}{}{}{}{}",
        ansi_fg(color),
        "─".repeat(left),
        title,
        "─".repeat(right),
        ansi_reset()
    );
    queue!(out, MoveTo(x, y), Print(&line), ResetColor)
}

/// Format a status bar like Hermes: `model | ctx -- | [████░░░░░░] -- | 38s | [OK]`
fn format_status_bar(
    model: &str,
    is_streaming: bool,
    stream_start: Option<std::time::Instant>,
    _tokens: u64,
) -> String {
    let cyan = ansi_fg(Color::Rgb { r: 0, g: 255, b: 255 });
    let gold = ansi_fg(Color::Rgb { r: 255, g: 215, b: 0 });
    let green = ansi_fg(Color::Rgb { r: 80, g: 255, b: 120 });
    let muted = ansi_fg(Color::Rgb { r: 140, g: 120, b: 160 });
    let reset = ansi_reset();

    let model_short = model.split('/').last().unwrap_or(model);
    let model_part = format!("{}{}{}", cyan, model_short, reset);

    let ctx_part = format!("{}ctx --{}", muted, reset);

    let progress = if is_streaming {
        let elapsed = stream_start.map(|s| s.elapsed().as_secs()).unwrap_or(0);
        let bars = (elapsed % 10) as usize;
        let filled = "█".repeat(bars);
        let empty = "░".repeat(10 - bars);
        format!("{}[{}{}]{} {}", green, filled, empty, reset, format_elapsed(stream_start))
    } else {
        format!("{}[░░░░░░░░░░]{} --", muted, reset)
    };

    let status = if is_streaming {
        format!("{}[STREAM]{}", gold, reset)
    } else {
        format!("{}[OK]{}", green, reset)
    };

    format!("{} | {} | {} | {}", model_part, ctx_part, progress, status)
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
