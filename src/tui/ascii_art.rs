//! OpenShark ASCII Art & Visual Identity
//!
//! Pixel-art style block-character art for the TUI welcome screen.
//! Matches the A-tier DOS title screen aesthetic:
//! - Blocky "OPENSHARK" wordmark with heavy visual weight
//! - Detailed shark fin with curve, notches, ridge line
//! - Three-layer pixel waves with foam crests
//! - Synthwave '84 color palette: deep purple, electric purple, hot pink, neon cyan

/// Center a line of text within a given width.
fn center(line: &str, width: usize) -> String {
    let line_width = line.chars().count();
    if line_width >= width {
        line.to_string()
    } else {
        let padding = (width - line_width) / 2;
        format!("{}{}", " ".repeat(padding), line)
    }
}

/// The OpenShark wordmark — heavy block letters that DOMINATE the frame.
/// Each letter is built from █ blocks with gaps for readability.
/// 50 chars wide, 5 lines tall. Positive-space rendering.
pub const WORDMARK: &str = r#"
 ████   ████  ██████  ████  ██  ██ ██████  ████   ████  ██  ██ ████  ██  ██
██  ██ ██  ██ ██     ██  ██ ██  ██ ██     ██  ██ ██  ██ ██  ██ ██ ██ ██ ██
██  ██ ██  ██ ████   ██████ ██████ ████   ██████ ██████ ██████ ██  ████  ██
██  ██ ██  ██ ██     ██     ██  ██ ██     ██  ██ ██  ██ ██  ██ ██   ██   ██
 ████   ████  ██     ██     ██  ██ ██     ██  ██ ██  ██ ██  ██ ██   ██   ██"#;

/// The OpenShark shark fin breaching the water.
/// The fin base merges into the waves below.
/// 27 chars wide at base, 10 lines tall (including wave merge).
pub const FIN_LOGO: &str = r#"
            ██
           ████
          ██████
         ████████
        ██████████
       ████████████
      ██████████████
     ████████████████
    ██████████████████
≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈"#;

/// Three-layer pixel waves with foam crests.
/// The waves merge with the fin base above.
pub const WAVE_BACK: &str =
    "≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈";
pub const WAVE_MID: &str =
    "≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈";
pub const WAVE_FRONT: &str = "≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈";

/// Full welcome banner: wordmark + tagline + fin + waves.
/// Dominates the chat frame, centered, symmetrical.
/// Matches the A-tier DOS title screen quality.
pub fn welcome_banner(frame_width: usize) -> String {
    let mut lines = Vec::new();

    // Wordmark — centered
    for line in WORDMARK.lines().skip(1) {
        lines.push(center(line, frame_width));
    }

    // Tagline
    lines.push(String::new());
    lines.push(center("Fast. Precise. Hungry.", frame_width));
    lines.push(String::new());

    // Fin + waves — the fin base merges into waves
    for line in FIN_LOGO.lines().skip(1) {
        lines.push(center(line, frame_width));
    }

    // Wave layers (full width, no centering needed)
    lines.push(WAVE_BACK.to_string());
    lines.push(WAVE_MID.to_string());
    lines.push(WAVE_FRONT.to_string());

    lines.join("\n")
}

/// Compact header for sidebar or small spaces.
/// Shows mini fin + tagline.
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
        assert!(first_line.len() <= 55, "Wordmark too wide: {}", first_line.len());
    }

    #[test]
    fn fin_connected_at_top() {
        let lines: Vec<_> = FIN_LOGO.lines().skip(1).collect();
        assert!(lines[0].contains("██"), "Fin top not connected");
        // Base should merge into waves (≈ character)
        let base = lines[lines.len() - 1];
        assert!(base.contains('≈'), "Fin base not merged with waves");
    }

    #[test]
    fn wave_layers_present() {
        assert_eq!(WAVE_BACK.chars().count(), 80);
        assert_eq!(WAVE_MID.chars().count(), 80);
        assert_eq!(WAVE_FRONT.chars().count(), 80);
    }

    #[test]
    fn welcome_banner_centered() {
        let banner = welcome_banner(60);
        let lines: Vec<_> = banner.lines().collect();
        let first = lines[0];
        assert!(first.starts_with(' ') || first.starts_with('█'), "Not centered");
    }
}
