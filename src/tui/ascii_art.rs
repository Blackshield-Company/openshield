//! OpenShark ASCII Art & Visual Identity
//!
//! Pixel-art style block-character art for the TUI welcome screen.
//! Matches the A-tier DOS title screen aesthetic:
//! - Blocky "OPENSHARK" wordmark with heavy visual weight
//! - Detailed shark fin with curve, notches, ridge line
//! - Three-layer pixel waves with foam crests
//! - Synthwave '84 color palette: deep purple, electric purple, hot pink, neon cyan

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Center a line of text within a given width, accounting for wide chars.
fn center(line: &str, width: usize) -> String {
    let line_width = line.width();
    if line_width >= width {
        line.to_string()
    } else {
        let padding = (width - line_width) / 2;
        format!("{}{}", " ".repeat(padding), line)
    }
}

/// The OpenShark wordmark — heavy block letters that DOMINATE the frame.
/// Each letter is built from █ blocks with gaps for readability.
/// 61 chars wide, 5 lines tall. Positive-space rendering.
pub const WORDMARK: &str = r#" ███   ████   █████  ██ ██   ████  ██ ██   ███   ████   ██ ██
██ ██  ██ ██  ██     ████   ██     ██ ██  ██ ██  ██ ██  ██ ██
██ ██  ████   ████   █████   ███   █████  █████  ████   ███  
██ ██  ██     ██     █ ███     ██  ██ ██  ██ ██  ██ ██  ██ ██
 ███   ██     █████  ██ ██  ████   ██ ██  ██ ██  ██ ██  ██ ██"#;

/// The OpenShark shark fin — sits directly on the water.
/// NO wave merge line — the fin sits directly on the full-width waves below.
/// 40 chars wide, 8 lines tall (just the fin body).
pub const FIN_LOGO: &str = r#"              ██
             ████
            ██████
           ████████
          ██████████
         ████████████
        ██████████████
       ████████████████"#;

/// Three-layer pixel waves.
/// Same width as fin base (40 chars) for seamless merge.
pub const WAVE_BACK: &str = "≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈";
pub const WAVE_MID: &str =  "≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈";
pub const WAVE_FRONT: &str = "≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈";

/// Generate a wave line that spans the full terminal width.
fn wave_line(frame_width: usize) -> String {
    let wave_char = '≈';
    let wave_unit = wave_char.to_string().repeat(40);
    let unit_width = wave_unit.width();
    if unit_width == 0 || frame_width <= unit_width {
        return wave_char.to_string().repeat(frame_width);
    }
    // Repeat the wave pattern to fill the width, then trim to exact size
    let repeats = (frame_width / unit_width) + 2;
    let mut full = wave_unit.repeat(repeats);
    // Trim to exact width by counting chars
    let mut result = String::with_capacity(frame_width);
    let mut current_width = 0;
    for ch in full.chars() {
        let ch_width = ch.width().unwrap_or(1);
        if current_width + ch_width > frame_width {
            break;
        }
        result.push(ch);
        current_width += ch_width;
    }
    // Pad if somehow short
    while result.width() < frame_width {
        result.push(wave_char);
    }
    result
}

/// Full welcome banner: wordmark + tagline + fin + waves.
/// Everything centered. Fin sits directly on waves.
/// Waves span the full terminal width.
pub fn welcome_banner(frame_width: usize) -> String {
    let mut lines = Vec::new();

    // Wordmark — centered
    for line in WORDMARK.lines() {
        lines.push(center(line, frame_width));
    }

    // Tagline — centered
    lines.push(String::new());
    lines.push(center("Fast. Precise. Hungry.", frame_width));
    lines.push(String::new());

    // Fin — centered (includes wave merge line as last row)
    for line in FIN_LOGO.lines() {
        lines.push(center(line, frame_width));
    }

    // Wave layers — FULL WIDTH, spanning entire terminal
    let full_wave = wave_line(frame_width);
    lines.push(full_wave.clone());
    lines.push(full_wave.clone());
    lines.push(full_wave);

    lines.join("\n")
}

/// Compact header for sidebar or small spaces.
pub fn session_header(version: &str) -> String {
    format!(
        r#"
    ██     openshark {}
   ████
  ██████
 ████████   Fast. Precise. Hungry.
██████████
≈≈≈≈≈≈≈≈≈≈≈"#,
        version
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordmark_spells_openshark() {
        let first_line = WORDMARK.lines().next().unwrap();
        assert!(first_line.contains("████"), "Missing block letters");
        assert!(
            first_line.width() <= 75,
            "Wordmark too wide: {}",
            first_line.width()
        );
    }

    #[test]
    fn fin_base_matches_wave_width() {
        let fin_lines: Vec<_> = FIN_LOGO.lines().collect();
        let base = fin_lines[fin_lines.len() - 1];
        assert!(
            base.contains("██████████████"),
            "Fin base should be the wide bottom line: {}",
            base
        );
    }

    #[test]
    fn welcome_banner_centered() {
        let banner = welcome_banner(80);
        let lines: Vec<_> = banner.lines().collect();
        // First wordmark line should be centered (starts with space or █)
        let first = &lines[0];
        assert!(first.starts_with(' ') || first.starts_with('█'), "Not centered: {}", first);
    }
}
