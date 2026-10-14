//! Config, data, and cache directories for OpenShield.
//!
//! Unix (Linux and macOS) follows the XDG Base Directory spec. Windows keeps
//! the `dirs` crate locations. Environment lookup is injectable so tests do
//! not mutate the process environment.

use std::path::{Path, PathBuf};

const APP: &str = "openshield";
const MIGRATED_MARKER: &str = ".xdg-migrated";

#[derive(Clone, Copy)]
enum Base {
    Config,
    Data,
    Cache,
}

impl Base {
    fn xdg_var(self) -> &'static str {
        match self {
            Self::Config => "XDG_CONFIG_HOME",
            Self::Data => "XDG_DATA_HOME",
            Self::Cache => "XDG_CACHE_HOME",
        }
    }

    fn fallback_rel(self) -> &'static str {
        match self {
            Self::Config => ".config",
            Self::Data => ".local/share",
            Self::Cache => ".cache",
        }
    }
}

/// `$XDG_CONFIG_HOME/openshield`, or `~/.config/openshield` on unix.
/// Windows: the `dirs` crate config dir plus `openshield`.
pub fn config_dir() -> Option<PathBuf> {
    configured_dir(Base::Config)
}

/// `$XDG_DATA_HOME/openshield`, or `~/.local/share/openshield` on unix.
/// Windows: the `dirs` crate data dir plus `openshield`.
pub fn data_dir() -> Option<PathBuf> {
    configured_dir(Base::Data)
}

/// `$XDG_CACHE_HOME/openshield`, or `~/.cache/openshield` on unix.
/// Windows: the `dirs` crate cache dir plus `openshield`.
pub fn cache_dir() -> Option<PathBuf> {
    configured_dir(Base::Cache)
}

#[cfg(any(windows, target_os = "android"))]
fn configured_dir(base: Base) -> Option<PathBuf> {
    let root = match base {
        Base::Config => dirs::config_dir(),
        Base::Data => dirs::data_dir(),
        Base::Cache => dirs::cache_dir(),
    };
    root.map(|path| path.join(APP))
}

#[cfg(all(unix, not(target_os = "android")))]
fn configured_dir(base: Base) -> Option<PathBuf> {
    base_dir_from(
        base,
        |key| std::env::var(key).ok(),
        dirs::home_dir().as_deref(),
    )
}

/// Resolve a user-supplied path. The portable default
/// `~/.config/openshield/...` is resolved against [`config_dir`] so
/// `XDG_CONFIG_HOME` is honored; anything else is tilde-expanded.
pub fn resolve_user_path(path: &str) -> PathBuf {
    const PREFIX: &str = "~/.config/openshield";
    if let Some(rest) = path.strip_prefix(PREFIX)
        && (rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'))
        && let Some(dir) = config_dir()
    {
        let rest = rest.trim_start_matches(['/', '\\']);
        return if rest.is_empty() { dir } else { dir.join(rest) };
    }
    PathBuf::from(shellexpand::tilde(path).as_ref())
}

/// Move legacy macOS `dirs` locations (and data files that used to live in
/// the config dir) into the XDG directories. Never fatal: errors are logged.
/// Runs once, marked by `data_dir/.xdg-migrated`. Does not overwrite an
/// existing destination. On unix the old path is left as a symlink, or a
/// `MOVED_TO_XDG.txt` note if the symlink cannot be created.
pub fn migrate_legacy() {
    #[cfg(not(all(unix, not(target_os = "android"))))]
    {
        return;
    }
    #[cfg(all(unix, not(target_os = "android")))]
    if let Err(err) = migrate_legacy_in(|key| std::env::var(key).ok(), dirs::home_dir().as_deref())
    {
        tracing::warn!("XDG path migration failed: {err}");
    }
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn config_dir_from(env: impl Fn(&str) -> Option<String>, home: Option<&Path>) -> Option<PathBuf> {
    base_dir_from(Base::Config, env, home)
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn data_dir_from(env: impl Fn(&str) -> Option<String>, home: Option<&Path>) -> Option<PathBuf> {
    base_dir_from(Base::Data, env, home)
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn cache_dir_from(env: impl Fn(&str) -> Option<String>, home: Option<&Path>) -> Option<PathBuf> {
    base_dir_from(Base::Cache, env, home)
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn base_dir_from(
    base: Base,
    env: impl Fn(&str) -> Option<String>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(value) = env(base.xdg_var()) {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Some(path.join(APP));
        }
    }
    home.map(|home| home.join(base.fallback_rel()).join(APP))
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn migrate_legacy_in(
    env: impl Fn(&str) -> Option<String>,
    home: Option<&Path>,
) -> std::io::Result<()> {
    let Some(home) = home else {
        return Ok(());
    };
    let Some(data) = data_dir_from(&env, Some(home)) else {
        return Ok(());
    };
    let marker = data.join(MIGRATED_MARKER);
    if path_exists(&marker) {
        return Ok(());
    }
    let config = config_dir_from(&env, Some(home));
    let cache = cache_dir_from(&env, Some(home));

    if let Some(cache) = &cache {
        for src in [
            home.join("Library/Caches").join(APP),
            home.join(".cache").join(APP),
        ] {
            move_tree(&src, cache)?;
        }
    }
    move_tree(&home.join(".local/share").join(APP), &data)?;

    let config_roots = [
        home.join("Library/Application Support").join(APP),
        home.join(".config").join(APP),
    ];
    let mut config_sources: Vec<PathBuf> = config_roots.to_vec();
    if let Some(cfg) = &config {
        if !config_sources.iter().any(|path| path == cfg) {
            config_sources.push(cfg.clone());
        }
        for src in &config_sources {
            relocate_data_entries(src, &data)?;
        }
        for src in &config_roots {
            move_tree(src, cfg)?;
        }
    } else {
        for src in &config_sources {
            relocate_data_entries(src, &data)?;
        }
    }

    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&marker, "xdg\n")?;
    Ok(())
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn relocate_data_entries(from: &Path, to: &Path) -> std::io::Result<()> {
    if from == to || real_metadata(from).is_none() {
        return Ok(());
    }
    for name in data_entry_names(from) {
        move_tree(&from.join(&name), &to.join(name))?;
    }
    Ok(())
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn data_entry_names(dir: &Path) -> Vec<String> {
    let mut names = vec![
        "memory.db".to_string(),
        "memory.db-wal".to_string(),
        "memory.db-shm".to_string(),
        "memory.db-journal".to_string(),
        "sessions".to_string(),
        "openshield.log".to_string(),
        "cronjobs".to_string(),
        "memories".to_string(),
    ];
    let Ok(entries) = std::fs::read_dir(dir) else {
        return names;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "code_index.db" || name.starts_with("code_index.db-") {
            continue;
        }
        let is_db = name.ends_with(".db")
            || name.ends_with(".db-wal")
            || name.ends_with(".db-shm")
            || name.ends_with(".db-journal");
        if is_db && !names.iter().any(|existing| existing == &name) {
            names.push(name);
        }
    }
    names
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn move_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src == dst || real_metadata(src).is_none() || path_exists(dst) {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    move_path(src, dst)?;
    leave_redirect(src, dst);
    Ok(())
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn move_path(src: &Path, dst: &Path) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    if path_exists(dst) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination exists",
        ));
    }
    if let Err(err) = copy_recursive(src, dst) {
        let _ = remove_path(dst);
        return Err(err);
    }
    remove_path(src)?;
    Ok(())
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            let target = std::fs::read_link(src)?;
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            return std::os::unix::fs::symlink(&target, dst);
        }
        #[cfg(not(unix))]
        {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            return std::fs::copy(src, dst).map(|_| ());
        }
    }
    if meta.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dst).map(|_| ())
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn remove_path(path: &Path) -> std::io::Result<()> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn leave_redirect(src: &Path, dst: &Path) {
    #[cfg(unix)]
    if let Err(err) = std::os::unix::fs::symlink(dst, src) {
        tracing::warn!(
            "failed to symlink {} -> {}: {err}; writing MOVED_TO_XDG.txt",
            src.display(),
            dst.display()
        );
        if let Err(note_err) = write_moved_note(src, dst) {
            tracing::warn!("failed to write MOVED_TO_XDG.txt: {note_err}");
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (src, dst);
    }
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
#[cfg(unix)]
fn write_moved_note(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::io::Write;

    let note = if src.is_dir() {
        src.join("MOVED_TO_XDG.txt")
    } else if let Some(parent) = src.parent() {
        parent.join("MOVED_TO_XDG.txt")
    } else {
        PathBuf::from("MOVED_TO_XDG.txt")
    };
    if let Some(parent) = note.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&note)?;
    writeln!(file, "{} -> {}", src.display(), dst.display())?;
    Ok(())
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn path_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

#[cfg(any(test, all(unix, not(target_os = "android"))))]
fn real_metadata(path: &Path) -> Option<std::fs::Metadata> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() {
        None
    } else {
        Some(meta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn xdg_env_honored_when_absolute() {
        let home = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let env = |key: &str| match key {
            "XDG_CONFIG_HOME" => Some(config.path().to_string_lossy().into_owned()),
            "XDG_DATA_HOME" => Some(data.path().to_string_lossy().into_owned()),
            "XDG_CACHE_HOME" => Some(cache.path().to_string_lossy().into_owned()),
            _ => None,
        };
        assert_eq!(
            config_dir_from(env, Some(home.path())),
            Some(config.path().join("openshield"))
        );
        assert_eq!(
            data_dir_from(env, Some(home.path())),
            Some(data.path().join("openshield"))
        );
        assert_eq!(
            cache_dir_from(env, Some(home.path())),
            Some(cache.path().join("openshield"))
        );
    }

    #[test]
    fn relative_xdg_value_ignored() {
        let home = tempfile::tempdir().unwrap();
        let env = |key: &str| match key {
            "XDG_CONFIG_HOME" => Some("relative/config".to_string()),
            "XDG_DATA_HOME" => Some("./relative/data".to_string()),
            "XDG_CACHE_HOME" => Some("cache".to_string()),
            _ => None,
        };
        assert_eq!(
            config_dir_from(env, Some(home.path())),
            Some(home.path().join(".config/openshield"))
        );
        assert_eq!(
            data_dir_from(env, Some(home.path())),
            Some(home.path().join(".local/share/openshield"))
        );
        assert_eq!(
            cache_dir_from(env, Some(home.path())),
            Some(home.path().join(".cache/openshield"))
        );
    }

    #[test]
    fn fallback_without_xdg_env() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            config_dir_from(no_env, Some(home.path())),
            Some(home.path().join(".config/openshield"))
        );
        assert_eq!(
            data_dir_from(no_env, Some(home.path())),
            Some(home.path().join(".local/share/openshield"))
        );
        assert_eq!(
            cache_dir_from(no_env, Some(home.path())),
            Some(home.path().join(".cache/openshield"))
        );
        assert_eq!(config_dir_from(no_env, None), None);
    }

    #[test]
    fn migration_moves_file_and_dir() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".config/openshield");
        std::fs::create_dir_all(config.join("sessions")).unwrap();
        std::fs::write(config.join("memory.db"), b"mem").unwrap();
        std::fs::write(config.join("sessions/s.json"), b"{}").unwrap();
        std::fs::write(config.join("config.toml"), b"keep").unwrap();

        let xdg_cache = tempfile::tempdir().unwrap();
        let old_cache = home.path().join(".cache/openshield");
        std::fs::create_dir_all(&old_cache).unwrap();
        std::fs::write(old_cache.join("response_cache.json"), b"c").unwrap();

        let env = |key: &str| {
            if key == "XDG_CACHE_HOME" {
                Some(xdg_cache.path().to_string_lossy().into_owned())
            } else {
                None
            }
        };
        migrate_legacy_in(env, Some(home.path())).unwrap();

        let data = home.path().join(".local/share/openshield");
        assert_eq!(std::fs::read(data.join("memory.db")).unwrap(), b"mem");
        assert_eq!(std::fs::read(data.join("sessions/s.json")).unwrap(), b"{}");
        assert_eq!(std::fs::read(config.join("config.toml")).unwrap(), b"keep");
        assert_eq!(
            std::fs::read(xdg_cache.path().join("openshield/response_cache.json")).unwrap(),
            b"c"
        );
        #[cfg(unix)]
        {
            assert!(
                config
                    .join("memory.db")
                    .symlink_metadata()
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert!(
                config
                    .join("sessions")
                    .symlink_metadata()
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert!(
                old_cache
                    .symlink_metadata()
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        assert!(data.join(".xdg-migrated").is_file());
    }

    #[test]
    fn migration_moves_macos_library_paths() {
        let home = tempfile::tempdir().unwrap();
        let app_support = home.path().join("Library/Application Support/openshield");
        std::fs::create_dir_all(&app_support).unwrap();
        std::fs::write(app_support.join("config.toml"), b"cfg").unwrap();
        std::fs::write(app_support.join("memory.db"), b"db").unwrap();
        std::fs::create_dir_all(app_support.join("sessions")).unwrap();
        std::fs::write(app_support.join("sessions/a.json"), b"{}").unwrap();
        let caches = home.path().join("Library/Caches/openshield");
        std::fs::create_dir_all(&caches).unwrap();
        std::fs::write(caches.join("response_cache.json"), b"cache").unwrap();

        migrate_legacy_in(no_env, Some(home.path())).unwrap();

        assert_eq!(
            std::fs::read(home.path().join(".config/openshield/config.toml")).unwrap(),
            b"cfg"
        );
        assert_eq!(
            std::fs::read(home.path().join(".local/share/openshield/memory.db")).unwrap(),
            b"db"
        );
        assert_eq!(
            std::fs::read(home.path().join(".local/share/openshield/sessions/a.json")).unwrap(),
            b"{}"
        );
        assert_eq!(
            std::fs::read(home.path().join(".cache/openshield/response_cache.json")).unwrap(),
            b"cache"
        );
        #[cfg(unix)]
        {
            assert!(
                app_support
                    .symlink_metadata()
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert!(caches.symlink_metadata().unwrap().file_type().is_symlink());
        }
    }

    #[test]
    fn migration_does_not_overwrite_existing_destination() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".config/openshield");
        let data = home.path().join(".local/share/openshield");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(config.join("memory.db"), b"legacy").unwrap();
        std::fs::write(data.join("memory.db"), b"current").unwrap();

        let old_cache = home.path().join("Library/Caches/openshield");
        let new_cache = home.path().join(".cache/openshield");
        std::fs::create_dir_all(&old_cache).unwrap();
        std::fs::create_dir_all(&new_cache).unwrap();
        std::fs::write(old_cache.join("a.txt"), b"old").unwrap();
        std::fs::write(new_cache.join("a.txt"), b"new").unwrap();

        migrate_legacy_in(no_env, Some(home.path())).unwrap();

        assert_eq!(std::fs::read(data.join("memory.db")).unwrap(), b"current");
        assert_eq!(std::fs::read(config.join("memory.db")).unwrap(), b"legacy");
        assert_eq!(std::fs::read(new_cache.join("a.txt")).unwrap(), b"new");
        assert_eq!(std::fs::read(old_cache.join("a.txt")).unwrap(), b"old");
    }

    #[test]
    fn migration_is_idempotent_via_marker() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".config/openshield");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("memory.db"), b"one").unwrap();
        migrate_legacy_in(no_env, Some(home.path())).unwrap();

        let data = home.path().join(".local/share/openshield");
        assert!(data.join(".xdg-migrated").is_file());
        std::fs::create_dir_all(config.join("sessions")).unwrap();
        std::fs::write(config.join("sessions/late.json"), b"late").unwrap();
        migrate_legacy_in(no_env, Some(home.path())).unwrap();

        assert!(config.join("sessions/late.json").is_file());
        assert!(!data.join("sessions").exists());
    }
}
