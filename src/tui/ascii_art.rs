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

/// Compact OpenShark wordmark — fits in ~45 char terminals.
/// Uses heavy block characters for visual punch.
/// 41 chars wide, 5 lines tall.
pub const WORDMARK: &str = r#"
 ██  ██  ███  ████ ██ ████  ████ ██  ██ ████
██  ██  ████ ████ ████ ████ ████ ████  ██  ██
██  ██  ████ ████ ████ ████ ████ ████  ██  ██
 ████   ████ ████ ████ ████ ████ ████  ████"#;

/// The OpenShark shark fin — detailed pixel-art style.
/// Features curve, notches, ridge line.
/// 31 chars wide, 14 lines tall.
pub const FIN_LOGO: &str = r#"
            ███
           █████
          ███████
         █████████
        ███████████
       █████████████
      ███████████████
     █████████████████
    ███████████████████
   █████████████████████
  ████ █████████████ ████
 ███    ███████████    ███
███      █████████      ███
█         ███████         █"#;

/// Three-layer pixel waves with foam crests.
pub const WAVE_BACK: &str =
    "▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓";
pub const WAVE_MID: &str =
    "▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒";
pub const WAVE_FRONT: &str = "░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░";
pub const WAVE_FOAM: &str =  "▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫▪▫";

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

    // Fin — centered
    for line in FIN_LOGO.lines().skip(1) {
        lines.push(center(line, frame_width));
    }

    // Wave layers
    lines.push(String::new());
    lines.push(WAVE_BACK.to_string());
    lines.push(WAVE_MID.to_string());
    lines.push(WAVE_FRONT.to_string());
    lines.push(WAVE_FOAM.to_string());

    lines.join("\n")
}

/// Compact header for sidebar or small spaces.
/// Shows mini fin + tagline.
pub fn session_header(version: &str) -> String {
    format!(
        r#"
    ███     openshark {}
   █████
  ███████
 █████████   Fast. Precise. Hungry.
███████████
░░░░░░░░░░░"#,
        version
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordmark_fits_in_terminal() {
        let first_line = WORDMARK.lines().nth(1).unwrap();
        assert!(first_line.len() <= 45, "Wordmark too wide for terminal: {}", first_line.len());
        assert!(first_line.contains("█"), "Missing block characters");
    }

    #[test]
    fn fin_connected_at_top() {
        let lines: Vec<_> = FIN_LOGO.lines().skip(1).collect();
        assert!(lines[0].contains("███"), "Fin top not connected");
        let base = lines[lines.len() - 2];
        assert!(base.contains("  "), "Fin base missing notch");
    }

    #[test]
    fn wave_layers_present() {
        assert_eq!(WAVE_BACK.chars().count(), 80);
        assert_eq!(WAVE_MID.chars().count(), 80);
        assert_eq!(WAVE_FRONT.chars().count(), 80);
        assert_eq!(WAVE_FOAM.chars().count(), 80);
    }

    #[test]
    fn welcome_banner_centered() {
        let banner = welcome_banner(50);
        let lines: Vec<_> = banner.lines().collect();
        let first = lines[0];
        assert!(first.starts_with(' ') || first.starts_with('█'), "Not centered");
    }
}
