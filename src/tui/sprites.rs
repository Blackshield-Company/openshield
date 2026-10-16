//! Pluggable mascot-sprite system.
//!
//! A sprite is a named, possibly animated piece of ASCII art rendered on the
//! splash screen beside the OpenShield banner. Sprite files live one-per-file:
//!
//! - **Stock sprites** ship in the repo under `assets/sprites/` and are
//!   embedded into the binary with `include_str!`.
//! - **User sprites** live in `~/.config/openshield/sprites/` (`*.ans` or
//!   `*.txt`) and override stock sprites of the same name.
//!
//! ## File format
//!
//! Each file holds one or more animation frames separated by a line
//! containing exactly `---frame---`. Lines starting with `#` are comments.
//! Frames are multi-line UTF-8 and may contain ANSI escapes.
//!
//! ## Theme tokens
//!
//! Instead of raw ANSI escapes, sprite files should use theme tokens, which
//! are replaced at load time with the *current* theme's ANSI escapes — this
//! keeps every sprite theme-compatible:
//!
//! `{fg}` `{bg}` `{accent}` `{accent_secondary}` `{accent_tertiary}`
//! `{muted}` `{blood}` (alias for `{accent}`) `{bone}` (alias for `{fg}`)
//! `{reset}`
//!
//! The name of the sprite is the filename minus its extension.

use std::path::{Path, PathBuf};

use crate::tui::theme::{ansi_bg, ansi_fg, ansi_reset, current_theme};

/// Frame separator line in sprite files.
const FRAME_SEP: &str = "---frame---";

/// The stock sprite that ships with OpenShield.
pub const STOCK_SPRITE: &str = "blackshield-mercenary";

/// Built-in sprites embedded into the binary: (name, file contents).
const BUILTIN_SPRITES: &[(&str, &str)] = &[(
    STOCK_SPRITE,
    include_str!("../../assets/sprites/blackshield-mercenary.ans"),
)];

/// A loaded mascot sprite: named, with one or more animation frames.
/// Each frame is a multi-line UTF-8 string; ANSI escapes are pre-baked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub name: String,
    pub frames: Vec<String>,
}

impl Sprite {
    /// Number of animation frames (always >= 1 for a successfully parsed sprite).
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// Replace theme tokens with the current theme's ANSI escapes.
/// Longest names first so `{accent_secondary}` is not clobbered by `{accent}`.
fn apply_theme_tokens(content: &str) -> String {
    let theme = current_theme();
    let mut out = content.to_string();
    for (token, escape) in [
        ("{accent_secondary}", ansi_fg(theme.accent_secondary)),
        ("{accent_tertiary}", ansi_fg(theme.accent_tertiary)),
        ("{accent}", ansi_fg(theme.accent)),
        ("{blood}", ansi_fg(theme.accent)),
        ("{muted}", ansi_fg(theme.muted)),
        ("{bone}", ansi_fg(theme.fg)),
        ("{fg}", ansi_fg(theme.fg)),
        ("{bg}", ansi_bg(theme.bg)),
        ("{reset}", ansi_reset().to_string()),
    ] {
        out = out.replace(token, &escape);
    }
    out
}

/// Parse sprite file content into frames: split on the frame separator,
/// drop comments and blank lines around frames, then bake theme tokens.
/// Wholly-empty frames are discarded; if nothing survives, returns None.
fn parse_sprite(name: &str, content: &str) -> Option<Sprite> {
    let mut frames: Vec<String> = Vec::new();
    for raw_frame in content.split(FRAME_SEP) {
        let frame: String = raw_frame
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
            .trim_matches('\n')
            .to_string();
        if frame.trim().is_empty() {
            continue;
        }
        frames.push(apply_theme_tokens(&frame));
    }
    if frames.is_empty() {
        return None;
    }
    Some(Sprite {
        name: name.to_string(),
        frames,
    })
}

/// User sprite directory: `~/.config/openshield/sprites`.
fn user_sprites_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("openshield").join("sprites"))
}

/// Read a sprite file from a directory if it exists (`<name>.ans` then `<name>.txt`).
fn read_sprite_file(dir: &Path, name: &str) -> Option<String> {
    for ext in ["ans", "txt"] {
        let path = dir.join(format!("{name}.{ext}"));
        if let Ok(content) = std::fs::read_to_string(&path) {
            return Some(content);
        }
    }
    None
}

/// Look up a built-in sprite by name.
fn builtin_sprite(name: &str) -> Option<&'static str> {
    BUILTIN_SPRITES
        .iter()
        .find(|(builtin_name, _)| *builtin_name == name)
        .map(|(_, content)| *content)
}

/// Load a sprite by name. User-dir sprites override built-ins of the same
/// name. Returns None when the name matches neither.
pub fn load_sprite(name: &str) -> Option<Sprite> {
    load_sprite_with_user_dir(name, user_sprites_dir().as_deref())
}

/// Load a sprite given an explicit user-dir override (None = skip user dir).
/// Split out from [`load_sprite`] so tests can exercise override behavior
/// without touching the real config directory.
fn load_sprite_with_user_dir(name: &str, user_dir: Option<&Path>) -> Option<Sprite> {
    if let Some(dir) = user_dir
        && let Some(content) = read_sprite_file(dir, name)
        && let Some(sprite) = parse_sprite(name, &content)
    {
        return Some(sprite);
    }
    builtin_sprite(name).and_then(|content| parse_sprite(name, content))
}

/// Load the stock sprite. Always succeeds — it is embedded in the binary.
pub fn load_stock_sprite() -> Sprite {
    load_sprite_with_user_dir(STOCK_SPRITE, None)
        .expect("stock sprite is embedded and must always parse")
}

/// Resolve a sprite name from config: unknown names fall back to the stock
/// sprite with a warning message suitable for the chat/system log.
/// `"none"` is handled by the caller (it disables the sprite entirely).
pub fn load_sprite_or_stock(name: &str) -> (Sprite, Option<String>) {
    match load_sprite(name) {
        Some(sprite) => (sprite, None),
        None => (
            load_stock_sprite(),
            Some(format!(
                "⚠️ Unknown sprite '{}' in config — falling back to the stock sprite '{}'. \
                 Run /sprite to list available sprites.",
                name, STOCK_SPRITE
            )),
        ),
    }
}

/// List every available sprite name (built-ins + user dir), sorted.
pub fn list_sprites() -> Vec<String> {
    list_sprites_with_user_dir(user_sprites_dir().as_deref())
}

/// List sprites given an explicit user-dir override.
fn list_sprites_with_user_dir(user_dir: Option<&Path>) -> Vec<String> {
    let mut names: Vec<String> = BUILTIN_SPRITES
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
    if let Some(dir) = user_dir
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "ans" || e == "txt")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    const MULTI_FRAME: &str = "\
# comment line, ignored
{fg}frame one{reset}
line two
---frame---
{accent}frame two{reset}
---frame---
{muted}frame three{reset}
";

    #[test]
    fn multi_frame_parse() {
        let sprite = parse_sprite("test", MULTI_FRAME).expect("should parse");
        assert_eq!(sprite.name, "test");
        assert_eq!(sprite.frame_count(), 3);
        assert!(sprite.frames[0].contains("frame one"));
        assert!(sprite.frames[0].contains("line two"));
        assert!(sprite.frames[1].contains("frame two"));
        assert!(sprite.frames[2].contains("frame three"));
        // Comments must not leak into frames.
        assert!(!sprite.frames[0].contains("comment"));
    }

    #[test]
    fn token_replacement() {
        let sprite = parse_sprite("tokens", "{fg}A{blood}B{bone}C{muted}D{reset}E").unwrap();
        let theme = current_theme();
        let expected = format!(
            "{}A{}B{}C{}D{}E",
            ansi_fg(theme.fg),
            ansi_fg(theme.accent),
            ansi_fg(theme.fg),
            ansi_fg(theme.muted),
            ansi_reset()
        );
        assert_eq!(sprite.frames[0], expected);
        // Aliases resolve to the same escapes as their canonical tokens.
        assert!(sprite.frames[0].contains(&format!("{}B", ansi_fg(theme.accent))));
    }

    #[test]
    fn accent_secondary_token_not_clobbered() {
        let sprite = parse_sprite("t", "{accent_secondary}X{reset}").unwrap();
        let theme = current_theme();
        assert_eq!(
            sprite.frames[0],
            format!("{}X{}", ansi_fg(theme.accent_secondary), ansi_reset())
        );
    }

    #[test]
    fn unknown_sprite_falls_back_to_stock() {
        let (sprite, warning) = load_sprite_or_stock("definitely-not-a-sprite");
        assert_eq!(sprite.name, STOCK_SPRITE);
        assert!(!sprite.frames.is_empty());
        let warning = warning.expect("unknown name must produce a warning");
        assert!(warning.contains("definitely-not-a-sprite"));
    }

    #[test]
    fn stock_sprite_loads_and_has_animation_frames() {
        let sprite = load_stock_sprite();
        assert_eq!(sprite.name, STOCK_SPRITE);
        assert!(sprite.frame_count() >= 2, "stock sprite must animate");
    }

    #[test]
    fn stock_sprite_has_black_steel_footprint() {
        // 32x28 px knight drawn with half-blocks: 14 rows, at most 32 cells
        // wide, every frame the same height so the animation does not jump.
        use crate::tui::components::splash::{max_frame_height, sprite_max_frame_width};
        let sprite = load_stock_sprite();
        assert_eq!(max_frame_height(&sprite), 14);
        assert!(sprite_max_frame_width(&sprite) <= 32);
        for frame in &sprite.frames {
            assert_eq!(frame.lines().count(), 14);
        }
    }

    #[test]
    fn user_dir_overrides_builtin_name() {
        let dir =
            std::env::temp_dir().join(format!("openshield-sprites-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{STOCK_SPRITE}.ans")),
            "{accent}user override{reset}",
        )
        .unwrap();

        let sprite =
            load_sprite_with_user_dir(STOCK_SPRITE, Some(&dir)).expect("user sprite should load");
        assert_eq!(sprite.name, STOCK_SPRITE);
        assert!(
            sprite.frames[0].contains("user override"),
            "user dir must override the built-in sprite of the same name"
        );

        // Listing sees the override name and stays deduped.
        let names = list_sprites_with_user_dir(Some(&dir));
        assert_eq!(
            names.iter().filter(|n| *n == STOCK_SPRITE).count(),
            1,
            "overridden name must appear exactly once"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_file_parses_to_nothing() {
        assert!(parse_sprite("empty", "# only comments\n").is_none());
        assert!(parse_sprite("empty", "").is_none());
    }
}
