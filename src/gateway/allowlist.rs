//! Persisted per-platform user-ID allowlist for channels.
//!
//! Stored in its own file (default `~/.config/openshield/channels-allowlist.toml`,
//! mode 0600) so a doctor config rewrite cannot wipe it. Config-seeded IDs in
//! `[gateway.<platform>] allowed_users` are also honoured, but never written
//! back to `config.toml` by this module.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::gateway::platform::Platform;

/// Default path: `$XDG_CONFIG_HOME/openshield/channels-allowlist.toml`.
pub fn default_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("openshield")
        .join("channels-allowlist.toml")
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AllowlistFile {
    #[serde(default)]
    telegram: Vec<String>,
    #[serde(default)]
    discord: Vec<String>,
    #[serde(default)]
    slack: Vec<String>,
    #[serde(default)]
    matrix: Vec<String>,
    #[serde(default)]
    signal: Vec<String>,
    #[serde(default)]
    email: Vec<String>,
    #[serde(default)]
    whatsapp: Vec<String>,
}

impl AllowlistFile {
    fn bucket_mut(&mut self, platform: Platform) -> &mut Vec<String> {
        match platform {
            Platform::Telegram => &mut self.telegram,
            Platform::Discord => &mut self.discord,
            Platform::Slack => &mut self.slack,
            Platform::Matrix => &mut self.matrix,
            Platform::Signal => &mut self.signal,
            Platform::Email => &mut self.email,
            Platform::WhatsApp => &mut self.whatsapp,
        }
    }

    fn bucket(&self, platform: Platform) -> &[String] {
        match platform {
            Platform::Telegram => &self.telegram,
            Platform::Discord => &self.discord,
            Platform::Slack => &self.slack,
            Platform::Matrix => &self.matrix,
            Platform::Signal => &self.signal,
            Platform::Email => &self.email,
            Platform::WhatsApp => &self.whatsapp,
        }
    }
}

/// Thread-safe allowlist: file entries plus config-seeded IDs.
pub struct Allowlist {
    path: PathBuf,
    persist: bool,
    /// IDs seeded from config.toml (not written to the allowlist file).
    seeded: BTreeMap<Platform, BTreeSet<String>>,
    file: Mutex<AllowlistFile>,
}

/// Build an allowlist from config: path, persist flag, and seeded IDs.
pub fn from_config(config: &crate::config::Config) -> Result<Allowlist> {
    let path = if config.gateway.allowlist_path.trim().is_empty() {
        default_path()
    } else {
        PathBuf::from(shellexpand::tilde(&config.gateway.allowlist_path).as_ref())
    };
    let mut seeded = BTreeMap::new();
    seed(
        &mut seeded,
        Platform::Telegram,
        &config.gateway.telegram.allowed_users,
    );
    seed(
        &mut seeded,
        Platform::Discord,
        &config.gateway.discord.allowed_users,
    );
    Allowlist::load(path, config.gateway.persist_paired_users, seeded)
}

fn seed(map: &mut BTreeMap<Platform, BTreeSet<String>>, platform: Platform, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    map.insert(platform, ids.iter().cloned().collect());
}

impl Allowlist {
    /// Load from `path`, creating an empty file if it is missing.
    pub fn load(
        path: PathBuf,
        persist: bool,
        seeded: BTreeMap<Platform, BTreeSet<String>>,
    ) -> Result<Self> {
        let file = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?
        } else {
            AllowlistFile::default()
        };
        Ok(Self {
            path,
            persist,
            seeded,
            file: Mutex::new(file),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// True when this user may talk (or press approval buttons) on `platform`.
    pub fn is_allowed(&self, platform: Platform, user_id: &str) -> bool {
        if user_id.is_empty() {
            return false;
        }
        if self
            .seeded
            .get(&platform)
            .is_some_and(|s| s.contains(user_id))
        {
            return true;
        }
        let file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        file.bucket(platform).iter().any(|id| id == user_id)
    }

    /// Number of distinct allowlisted users for a platform (seeded + file).
    pub fn user_count(&self, platform: Platform) -> usize {
        let mut set: BTreeSet<String> = self.seeded.get(&platform).cloned().unwrap_or_default();
        let file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        for id in file.bucket(platform) {
            set.insert(id.clone());
        }
        set.len()
    }

    /// Persist `user_id` on the allowlist. No-op if already present.
    pub fn add(&self, platform: Platform, user_id: &str) -> Result<bool> {
        if user_id.is_empty() {
            anyhow::bail!("empty user id");
        }
        if self.is_allowed(platform, user_id) {
            return Ok(false);
        }
        if !self.persist {
            return Ok(false);
        }
        {
            let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
            let bucket = file.bucket_mut(platform);
            if !bucket.iter().any(|id| id == user_id) {
                bucket.push(user_id.to_string());
            }
            self.write_locked(&file)?;
        }
        Ok(true)
    }

    fn write_locked(&self, file: &AllowlistFile) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let body = toml::to_string_pretty(file).context("serializing allowlist")?;
        let tmp = self.path.with_extension("toml.tmp");
        {
            let mut f = std::fs::File::create(&tmp)
                .with_context(|| format!("creating {}", tmp.display()))?;
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("writing {}", self.path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "openshield-allowlist-{}-{n}.toml",
            std::process::id()
        ))
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("toml.tmp"));
    }

    #[test]
    fn empty_denies() {
        let path = temp_path();
        cleanup(&path);
        let list = Allowlist::load(path.clone(), true, BTreeMap::new()).unwrap();
        assert!(!list.is_allowed(Platform::Telegram, "100"));
        assert_eq!(list.user_count(Platform::Telegram), 0);
        cleanup(&path);
    }

    #[test]
    fn seeded_is_allowed_without_write() {
        let path = temp_path();
        cleanup(&path);
        let mut seeded = BTreeMap::new();
        seeded.insert(Platform::Telegram, BTreeSet::from(["42".into()]));
        let list = Allowlist::load(path.clone(), true, seeded).unwrap();
        assert!(list.is_allowed(Platform::Telegram, "42"));
        assert!(!list.is_allowed(Platform::Telegram, "99"));
        assert!(!path.exists(), "seeded IDs must not create the file");
        cleanup(&path);
    }

    #[test]
    fn add_persists_and_reloads() {
        let path = temp_path();
        cleanup(&path);
        {
            let list = Allowlist::load(path.clone(), true, BTreeMap::new()).unwrap();
            assert!(list.add(Platform::Telegram, "100").unwrap());
            assert!(!list.add(Platform::Telegram, "100").unwrap());
            assert!(list.is_allowed(Platform::Telegram, "100"));
        }
        let list = Allowlist::load(path.clone(), true, BTreeMap::new()).unwrap();
        assert!(list.is_allowed(Platform::Telegram, "100"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        cleanup(&path);
    }

    #[test]
    fn persist_false_does_not_write() {
        let path = temp_path();
        cleanup(&path);
        let list = Allowlist::load(path.clone(), false, BTreeMap::new()).unwrap();
        assert!(!list.add(Platform::Discord, "7").unwrap());
        assert!(!path.exists());
        cleanup(&path);
    }
}
