//! OpenShark ASCII Art & Visual Identity
//!
//! Custom block-character art for the TUI welcome screen and branding.
//! Designed for the synthwave '84 aesthetic — deep purples, electric cyan,
//! hot pink accents. All art fits within 80-column terminals.

/// The OpenShark dorsal fin logo.
/// A stylized shark fin rendered in half-block characters with gradient.
/// 16 chars wide, 8 lines tall — compact but distinctive.
pub const FIN_LOGO: &str = r#"
    ▗▄▄▄▄▄▄▄▄▖
   ▗█▀▀▀▀▀▀▀▀█▖
  ▗█▛  ▐█   ▜█▖
 ▗█▛   ▐█    ▜█▖
▗█▛    ▐█     ▜█▖
█▛     ▐█      ▜█
▀      ▐█       ▀
       ▐█"#;

/// Compact fin icon for inline use (sidebar header, etc).
/// 6 chars wide, 5 lines tall.
pub const FIN_ICON: &str = r#"
 ▄▄▄▄
█▀▀▀█
█  ▐█
█  ▐█
   ▐█"#;

/// The OpenShark wordmark in clean block letters.
/// Fits in 64 columns, 5 lines tall.
pub const WORDMARK: &str = r#"
 ██████  ██████  ██████  ██   ██ ███████  █████  ██████   ██ ██
██    ██ ██   ██ ██   ██ ██   ██ ██      ██   ██ ██   ██  ██ ██
██    ██ ██████  ██████  ███████ █████   ███████ ██████   ██ ██
██    ██ ██      ██   ██ ██   ██ ██      ██   ██ ██   ██  ██ ██
 ██████  ██      ██████  ██   ██ ███████ ██   ██ ██   ██  ██ ██"#;

/// Combined welcome banner: fin + wordmark.
pub fn welcome_banner() -> String {
    format!("{}{}", FIN_LOGO, WORDMARK)
}

/// Session startup header with version.
pub fn session_header(version: &str) -> String {
    format!(
        r#"
 ▄▄▄▄    openshark {}
█▀▀▀█
█  ▐█   Fast. Precise. Hungry.
█  ▐█
   ▐█"#,
        version
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fin_logo_has_expected_lines() {
        assert_eq!(FIN_LOGO.lines().count(), 9); // 8 art + 1 leading newline
    }

    #[test]
    fn wordmark_fits_in_80_cols() {
        for line in WORDMARK.lines() {
            assert!(line.len() <= 64, "Wordmark line too wide: {}", line);
        }
    }

    #[test]
    fn welcome_banner_combines() {
        let banner = welcome_banner();
        assert!(banner.contains("▄▄▄▄"));
        assert!(banner.contains("openshark"));
    }
}
