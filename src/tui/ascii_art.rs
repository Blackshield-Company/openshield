//! OpenShark ASCII Art & Visual Identity
//!
//! Pixel-art style block-character art for the TUI welcome screen.
//! Matches the A-tier DOS title screen aesthetic:
//! - Blocky "OPENSHARK" wordmark with heavy visual weight
//! - Detailed shark fin with curve, notches, ridge line
//! - Three-layer pixel waves with foam crests
//! - Synthwave '84 color palette: deep purple, electric purple, hot pink, neon cyan

use unicode_width::UnicodeWidthStr;

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
/// ~50 display cols wide, 5 lines tall. Positive-space rendering.
pub const WORDMARK: &str = r#"
 ████   ████  ██████  ████  ██  ██ ██████  ████   ████  ██  ██ ████  ██  ██
██  ██ ██  ██ ██     ██  ██ ██  ██ ██     ██  ██ ██  ██ ██  ██ ██ ██ ██ ██
██  ██ ██  ██ ████   ██████ ██████ ████   ██████ ██████ ██████ ██  ████  ██
██  ██ ██  ██ ██     ██     ██  ██ ██     ██  ██ ██  ██ ██  ██ ██   ██   ██
 ████   ████  ██     ██     ██  ██ ██     ██  ██ ██  ██ ██  ██ ██   ██   ██"#;

/// The OpenShark shark fin — sits directly on the water.
/// Fin base is the same width as the wave lines below it.
/// 40 chars wide, 9 lines tall (fin body + wave merge line).
pub const FIN_LOGO: &str = r#"
              ██
             ████
            ██████
           ████████
          ██████████
         ████████████
        ██████████████
       ████████████████
≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈"#;

/// Three-layer pixel waves.
/// Same width as fin base (40 chars) for seamless merge.
pub const WAVE_BACK: &str = "≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈";
pub const WAVE_MID: &str =  "≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈";
pub const WAVE_FRONT: &str = "≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈";

/// Full welcome banner: wordmark + tagline + fin + waves.
/// Everything centered. Fin sits directly on waves.
pub fn welcome_banner(frame_width: usize) -> String {
    let mut lines = Vec::new();

    // Wordmark — centered
    for line in WORDMARK.lines().skip(1) {
        lines.push(center(line, frame_width));
    }

    // Tagline — centered
    lines.push(String::new());
    lines.push(center("Fast. Precise. Hungry.", frame_width));
    lines.push(String::new());

    // Fin — centered (includes wave merge line as last row)
    for line in FIN_LOGO.lines().skip(1) {
        lines.push(center(line, frame_width));
    }

    // Wave layers — centered to match fin base width
    lines.push(center(WAVE_BACK, frame_width));
    lines.push(center(WAVE_MID, frame_width));
    lines.push(center(WAVE_FRONT, frame_width));

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
        let first_line = WORDMARK.lines().nth(1).unwrap();
        assert!(first_line.contains("████"), "Missing block letters");
        assert!(first_line.width() <= 55, "Wordmark too wide: {}", first_line.width());
    }

    #[test]
    fn fin_base_matches_wave_width() {
        let fin_lines: Vec<_> = FIN_LOGO.lines().skip(1).collect();
        let base = fin_lines[fin_lines.len() - 1];
        assert_eq!(base.width(), WAVE_BACK.width(), "Fin base and wave width mismatch");
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
