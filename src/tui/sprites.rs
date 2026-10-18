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
//! ## Animation states
//!
//! A file may group frames into named states with a line containing exactly
//! `---state:<name>---` (names: `idle`, `thinking`, `working`, `success`,
//! `error`). Frames after a marker belong to that state until the next one.
//! Files without markers are a single `idle` state, so older sprites keep
//! working unchanged. Unknown state names are ignored.
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

/// Animation state of the mascot, driven by what the agent is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteState {
    Idle,
    Thinking,
    Working,
    Success,
    Error,
}

impl SpriteState {
    /// Parse a state name from a `---state:<name>---` marker.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "idle" => Some(Self::Idle),
            "thinking" => Some(Self::Thinking),
            "working" => Some(Self::Working),
            "success" => Some(Self::Success),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Map agent activity to a sprite state: streaming with no output yet is
/// thinking, streaming output or running tools is working, otherwise idle.
pub fn agent_sprite_state(streaming: bool, has_output: bool, tools_active: bool) -> SpriteState {
    if tools_active || (streaming && has_output) {
        SpriteState::Working
    } else if streaming {
        SpriteState::Thinking
    } else {
        SpriteState::Idle
    }
}

/// Environment variables that request reduced motion. Any non-empty value
/// except `0`, `false`, `no` or `off` turns animation off.
pub const REDUCED_MOTION_ENV_VARS: &[&str] = &["NO_MOTION", "REDUCE_MOTION"];

/// True when an environment value asks for reduced motion.
pub fn env_value_requests_reduced_motion(value: Option<&str>) -> bool {
    match value.map(|v| v.trim().to_ascii_lowercase()) {
        None => false,
        Some(v) => !matches!(v.as_str(), "" | "0" | "false" | "no" | "off"),
    }
}

/// True when `NO_MOTION` or `REDUCE_MOTION` asks for reduced motion.
pub fn reduced_motion_from_env() -> bool {
    REDUCED_MOTION_ENV_VARS
        .iter()
        .any(|key| env_value_requests_reduced_motion(std::env::var(key).ok().as_deref()))
}

/// Argument to the `/motion` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionCommand {
    /// Show the current setting.
    Status,
    /// Animate the sprite (`/motion on`).
    Animate,
    /// Still frame only (`/motion off`, `/motion reduce`).
    Still,
    /// Flip the current setting.
    Toggle,
}

/// Parse the argument of `/motion`. Empty means status.
pub fn parse_motion_command(arg: &str) -> Option<MotionCommand> {
    match arg.trim().to_ascii_lowercase().as_str() {
        "" | "status" => Some(MotionCommand::Status),
        "on" | "animate" | "full" => Some(MotionCommand::Animate),
        "off" | "still" | "reduce" | "reduced" => Some(MotionCommand::Still),
        "toggle" => Some(MotionCommand::Toggle),
        _ => None,
    }
}

/// A loaded mascot sprite: named, with one or more animation frames.
/// Each frame is a multi-line UTF-8 string; ANSI escapes are pre-baked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub name: String,
    /// Idle frames (the first state when the file has no `idle` state).
    pub frames: Vec<String>,
    /// Frames per animation state, in file order.
    pub states: Vec<(SpriteState, Vec<String>)>,
}

impl Sprite {
    /// Frames for a state, falling back to the idle frames when the sprite
    /// does not define that state.
    pub fn frames_for(&self, state: SpriteState) -> &[String] {
        self.states
            .iter()
            .find(|(s, _)| *s == state)
            .map(|(_, f)| f.as_slice())
            .unwrap_or(&self.frames)
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

/// Parse one block of frames: split on the frame separator, drop comments
/// and blank lines around frames, then bake theme tokens. Wholly-empty
/// frames are discarded.
fn parse_frames(content: &str) -> Vec<String> {
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
    frames
}

/// Parse sprite file content into states and frames (see the module docs).
/// If no frames survive, returns None.
fn parse_sprite(name: &str, content: &str) -> Option<Sprite> {
    // Split into (state, block) sections on `---state:<name>---` lines.
    let mut sections: Vec<(Option<SpriteState>, String)> =
        vec![(Some(SpriteState::Idle), String::new())];
    for line in content.lines() {
        let t = line.trim();
        if let Some(rest) = t
            .strip_prefix("---state:")
            .and_then(|r| r.strip_suffix("---"))
        {
            sections.push((SpriteState::from_name(rest), String::new()));
            continue;
        }
        if let Some((_, block)) = sections.last_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    let mut states: Vec<(SpriteState, Vec<String>)> = Vec::new();
    for (state, block) in sections {
        let Some(state) = state else { continue };
        let frames = parse_frames(&block);
        if frames.is_empty() {
            continue;
        }
        match states.iter_mut().find(|(s, _)| *s == state) {
            Some((_, existing)) => existing.extend(frames),
            None => states.push((state, frames)),
        }
    }
    let frames = states
        .iter()
        .find(|(s, _)| *s == SpriteState::Idle)
        .or_else(|| states.first())
        .map(|(_, f)| f.clone())?;
    Some(Sprite {
        name: name.to_string(),
        frames,
        states,
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

    #[test]
    fn reduced_motion_env_values() {
        assert!(!env_value_requests_reduced_motion(None));
        for off in ["", " ", "0", "false", "FALSE", "no", "off"] {
            assert!(!env_value_requests_reduced_motion(Some(off)), "{off:?}");
        }
        for on in ["1", "true", "yes", "on", "reduce"] {
            assert!(env_value_requests_reduced_motion(Some(on)), "{on:?}");
        }
    }

    #[test]
    fn motion_command_parsing() {
        assert_eq!(parse_motion_command(""), Some(MotionCommand::Status));
        assert_eq!(parse_motion_command("status"), Some(MotionCommand::Status));
        assert_eq!(parse_motion_command("on"), Some(MotionCommand::Animate));
        assert_eq!(
            parse_motion_command("Animate"),
            Some(MotionCommand::Animate)
        );
        assert_eq!(parse_motion_command("off"), Some(MotionCommand::Still));
        assert_eq!(parse_motion_command("still"), Some(MotionCommand::Still));
        assert_eq!(parse_motion_command(" reduce "), Some(MotionCommand::Still));
        assert_eq!(parse_motion_command("toggle"), Some(MotionCommand::Toggle));
        assert_eq!(parse_motion_command("sideways"), None);
    }

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
        assert_eq!(sprite.frames.len(), 3);
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
        assert!(sprite.frames.len() >= 2, "stock sprite must animate");
    }

    #[test]
    fn parses_named_states() {
        let content = "# c\n---state:idle---\nA1\n---frame---\nA2\n---state:thinking---\nT1\n---state:bogus---\nX\n---state:error---\nE1\n";
        let sprite = parse_sprite("s", content).expect("parses");
        assert_eq!(sprite.frames.len(), 2);
        assert_eq!(sprite.frames_for(SpriteState::Thinking).len(), 1);
        assert!(sprite.frames_for(SpriteState::Error)[0].contains("E1"));
        // Missing state falls back to idle; unknown state is ignored.
        assert_eq!(
            sprite.frames_for(SpriteState::Success),
            sprite.frames.as_slice()
        );
        assert!(!sprite.frames.iter().any(|f| f.contains('X')));
    }

    #[test]
    fn files_without_states_are_idle() {
        let sprite = parse_sprite("s", "one\n---frame---\ntwo\n").expect("parses");
        assert_eq!(sprite.states.len(), 1);
        assert_eq!(sprite.states[0].0, SpriteState::Idle);
        assert_eq!(sprite.frames_for(SpriteState::Working).len(), 2);
    }

    #[test]
    fn agent_activity_maps_to_states() {
        assert_eq!(agent_sprite_state(false, false, false), SpriteState::Idle);
        assert_eq!(
            agent_sprite_state(true, false, false),
            SpriteState::Thinking
        );
        assert_eq!(agent_sprite_state(true, true, false), SpriteState::Working);
        assert_eq!(agent_sprite_state(false, false, true), SpriteState::Working);
    }

    #[test]
    fn stock_sprite_defines_every_state() {
        let sprite = load_stock_sprite();
        for state in ["idle", "thinking", "working", "success", "error"] {
            let st = SpriteState::from_name(state).expect("known state");
            let frames = sprite.frames_for(st);
            assert!(
                sprite.states.iter().any(|(s, _)| *s == st),
                "missing state {state}"
            );
            assert!((2..=6).contains(&frames.len()), "{state} frame count");
            for f in frames {
                assert_eq!(f.lines().count(), 14, "{state} frame height");
            }
        }
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
