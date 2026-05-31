//! OpenShark ASCII Art & Visual Identity
//!
//! Custom block-character art for the TUI welcome screen and branding.
//! Designed for the synthwave '84 aesthetic — deep purples, electric cyan,
//! hot pink accents. All art fits within 80-column terminals.
//!
//! Quality targets (inspired by Hermes caduceus):
//! - Braille/texture density for organic forms
//! - Gradient shading with block characters
//! - Asymmetric composition with dynamic balance
//! - Negative space mastery — form defined by surroundings

/// The OpenShark dorsal fin logo — high quality version.
///
/// Design principles:
/// - Forward tilt (15°) suggests motion through water
/// - Asymmetric trailing edge — longer behind, sharp at front
/// - Internal highlight stripe down leading edge
/// - Braille dots for water spray texture
/// - Fade below waterline — only suggestion, not definition
///
/// 34 chars wide, 12 lines tall.
pub const FIN_LOGO: &str = r#"
              ▗▄▄
             ▗████▖
            ▗██▓▓██▖
           ▗██▓░░▓██▖
          ▗██▓░  ░▓██▖
         ▗██▓░    ░▓██▖
        ▗██▓░      ░▓██▖
       ▗██▓░        ░▓██▖
      ▗██▓░          ░▓██▖
     ▗██▓░            ░▓██▖
    ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄
   ░░░░░░░░░░░░░░░░░░░░░░░░░░
  ░░⠑⠒⠐⠄⠆⠁░░░░░░░░░░░░░░░░░░░░░░░
 ░░⠁⠂⠄⠆⠇⠈⠉⠊⠋⠌⠍⠎⠏░░░░░░░░░░░░░░░░░░░░░"#;

/// Compact fin icon for inline use (sidebar header, etc).
/// 10 chars wide, 6 lines tall — distilled essence of the full logo.
pub const FIN_ICON: &str = r#"
    ▗▄
   ▗██▖
  ▗█▓▓█▖
 ▗█▓░░▓█▖
▗█▓░  ░▓█▖
▄▄▄▄▄▄▄▄▄▄"#;

/// The OpenShark wordmark in clean block letters.
/// Geometric, architectural — internal structure like Hermes title.
/// Fits in 64 columns, 5 lines tall.
pub const WORDMARK: &str = r#"
 ░█████░  ██████  ██████  ██   ██ ███████  █████  ██████   ██ ██
░██  ░██ ██  ░██ ██  ░██ ██   ██ ██      ██   ██ ██   ██  ██ ██
░██  ░██ ██████  ██████  ███████ █████   ███████ ██████   ██ ██
░██  ░██ ██  ░░  ██   ██ ██   ██ ██      ██   ██ ██   ██  ██ ██
 ░█████░  ██████  ██████  ██   ██ ███████ ██   ██ ██   ██  ██ ██"#;

/// Combined welcome banner: fin + wordmark side by side.
/// The fin provides organic counterweight to the geometric text.
pub fn welcome_banner() -> String {
    // Stack them vertically for cleaner composition
    format!("{}\n{}", FIN_LOGO, WORDMARK)
}

/// Session startup header — compact, iconic.
/// Uses the fin icon + text in a tight composition.
pub fn session_header(version: &str) -> String {
    format!(
        r#"
    ▗▄     openshark {}
   ▗██▖
  ▗█▓▓█▖   Fast. Precise. Hungry.
 ▗█▓░░▓█▖
▗█▓░  ░▓█▖
▄▄▄▄▄▄▄▄▄▄"#,
        version
    )
}

/// Water texture using Braille dots — for ambient background effects.
/// Same technique as Hermes caduceus: sparse points read as pattern.
pub const WATER_TEXTURE: &str = r#"⠑⠒⠐⠄⠆⠁⠁⠂⠄⠆⠇⠈⠉⠊⠋⠌⠍⠎⠏"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fin_logo_dimensions() {
        let lines: Vec<_> = FIN_LOGO.lines().collect();
        assert_eq!(lines.len(), 15); // 14 art + 1 leading newline
        let max_width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        assert!(max_width <= 40, "FIN_LOGO too wide: {} chars", max_width);
    }

    #[test]
    fn wordmark_fits_in_80_cols() {
        for line in WORDMARK.lines() {
            assert!(line.len() <= 66, "Wordmark line too wide: {}", line);
        }
    }

    #[test]
    fn session_header_contains_version() {
        let header = session_header("1.0.0");
        assert!(header.contains("openshark 1.0.0"));
        assert!(header.contains("Fast. Precise. Hungry."));
    }

    #[test]
    fn fin_icon_compact() {
        let lines: Vec<_> = FIN_ICON.lines().collect();
        assert_eq!(lines.len(), 7); // 6 art + 1 leading newline
    }
}
