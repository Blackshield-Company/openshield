use anyhow::{Context, Result};

// ANSI color codes (no external crate needed)
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";

/// Health check result for a single component.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub component: String,
    pub status: CheckStatus,
    pub message: String,
    pub fixable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckStatus {
    Healthy,
    Warning,
    Critical,
}

impl CheckResult {
    pub fn healthy(component: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            component: component.into(),
            status: CheckStatus::Healthy,
            message: message.into(),
            fixable: false,
        }
    }

    pub fn warning(
        component: impl Into<String>,
        message: impl Into<String>,
        fixable: bool,
    ) -> Self {
        Self {
            component: component.into(),
            status: CheckStatus::Warning,
            message: message.into(),
            fixable,
        }
    }

    pub fn critical(
        component: impl Into<String>,
        message: impl Into<String>,
        fixable: bool,
    ) -> Self {
        Self {
            component: component.into(),
            status: CheckStatus::Critical,
            message: message.into(),
            fixable,
        }
    }
}

/// Doctor report containing all check results.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub checks: Vec<CheckResult>,
    pub fixes_applied: Vec<String>,
}

impl DoctorReport {
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
            fixes_applied: Vec::new(),
        }
    }

    pub fn healthy_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == CheckStatus::Healthy)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == CheckStatus::Warning)
            .count()
    }

    pub fn critical_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == CheckStatus::Critical)
            .count()
    }

    pub fn print(&self) {
        println!("\n{BOLD}{CYAN}🛡 OpenShield Doctor{RESET}");
        println!("{CYAN}{}{RESET}", "═".repeat(60));

        for check in &self.checks {
            let (icon, status_str, status_color) = match check.status {
                CheckStatus::Healthy => ("✅", "HEALTHY", GREEN),
                CheckStatus::Warning => ("⚠️", "WARNING", YELLOW),
                CheckStatus::Critical => ("❌", "CRITICAL", RED),
            };
            println!(
                "{} {BOLD}{:<20}{RESET} [{status_color}{}{RESET}] {}",
                icon, check.component, status_str, check.message
            );
        }

        println!("{CYAN}{}{RESET}", "─".repeat(60));
        println!(
            "Summary: {GREEN}{}{RESET} {GREEN}healthy{RESET} | {YELLOW}{}{RESET} {YELLOW}warnings{RESET} | {RED}{}{RESET} {RED}critical{RESET}",
            self.healthy_count(),
            self.warning_count(),
            self.critical_count(),
        );

        if !self.fixes_applied.is_empty() {
            println!("\n🔧 Fixes applied:");
            for fix in &self.fixes_applied {
                println!("   ✅ {}", fix);
            }
        }

        if self.critical_count() > 0 {
            println!(
                "\n{BOLD}{RED}⚠️  Critical issues found. Run `openshield doctor --fix` to auto-repair.{RESET}"
            );
        } else if self.warning_count() > 0 {
            println!(
                "\n{YELLOW}💡 Warnings found. Run `openshield doctor --fix` to auto-repair.{RESET}"
            );
        } else {
            println!("\n{BOLD}{GREEN}🎉 All systems healthy!{RESET}");
        }
    }
}

impl Default for DoctorReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Run all health checks and return a report.
pub async fn run_checks(auto_fix: bool) -> Result<DoctorReport> {
    let mut report = DoctorReport::new();

    // Check 1: Config file
    report.checks.push(check_config().await);

    // Check 2: Providers / API keys
    report.checks.push(check_providers().await);

    // Check 3: Memory database
    report.checks.push(check_memory_db().await);

    // Check 4: Cache directory
    report.checks.push(check_cache().await);

    // Check 5: Skills directory
    report.checks.push(check_skills().await);

    // Check 6: Binary / build integrity
    report.checks.push(check_binary().await);

    // Check 7: Session exports directory
    report.checks.push(check_sessions_dir().await);

    // Auto-fix if requested
    if auto_fix {
        for check in &report.checks {
            if check.fixable
                && (check.status == CheckStatus::Warning || check.status == CheckStatus::Critical)
                && let Ok(fix_msg) = try_fix(&check.component).await
            {
                report.fixes_applied.push(fix_msg);
            }
        }
    }

    Ok(report)
}

async fn check_config() -> CheckResult {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield");
    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        return CheckResult::critical(
            "Config",
            format!("Config file not found at {}", config_path.display()),
            true,
        );
    }

    match std::fs::read_to_string(&config_path) {
        Ok(contents) => {
            if contents.trim().is_empty() {
                return CheckResult::critical("Config", "Config file is empty", true);
            }
            match toml::from_str::<serde_json::Value>(&contents) {
                Ok(_) => CheckResult::healthy(
                    "Config",
                    format!("Valid TOML at {}", config_path.display()),
                ),
                Err(e) => CheckResult::critical("Config", format!("Invalid TOML: {}", e), true),
            }
        }
        Err(e) => CheckResult::critical("Config", format!("Cannot read config: {}", e), false),
    }
}

async fn check_providers() -> CheckResult {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield");

    let env_files = ["kimi.env", "openai.env", "zai.env", "fal.env"];
    let mut found = 0;
    let mut total = 0;

    for file in &env_files {
        total += 1;
        let path = config_dir.join(file);
        if path.exists() {
            found += 1;
        }
    }

    if found == 0 {
        return CheckResult::warning(
            "Providers",
            "No API key env files found. Add keys to ~/.config/openshield/*.env",
            false,
        );
    }

    CheckResult::healthy(
        "Providers",
        format!("{}/{} API key files present", found, total),
    )
}

async fn check_memory_db() -> CheckResult {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield");
    let db_path = data_dir.join("memory.db");

    if !db_path.exists() {
        return CheckResult::warning(
            "Memory DB",
            format!(
                "Database not found at {}. Will be created on first use.",
                db_path.display()
            ),
            true,
        );
    }

    match rusqlite::Connection::open(&db_path) {
        Ok(conn) => match conn
            .query_row("PRAGMA integrity_check;", [], |row| row.get::<_, String>(0))
        {
            Ok(result) if result == "ok" => {
                CheckResult::healthy("Memory DB", format!("Database OK at {}", db_path.display()))
            }
            Ok(result) => CheckResult::critical(
                "Memory DB",
                format!("Integrity check failed: {}", result),
                true,
            ),
            Err(e) => {
                CheckResult::critical("Memory DB", format!("Corruption detected: {}", e), true)
            }
        },
        Err(e) => CheckResult::critical("Memory DB", format!("Cannot open: {}", e), true),
    }
}

async fn check_cache() -> CheckResult {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield");

    if !cache_dir.exists() {
        return CheckResult::warning(
            "Cache",
            format!(
                "Cache dir not found at {}. Will be created.",
                cache_dir.display()
            ),
            true,
        );
    }

    match std::fs::read_dir(&cache_dir) {
        Ok(entries) => {
            let count = entries.count();
            CheckResult::healthy("Cache", format!("{} entries in cache", count))
        }
        Err(e) => CheckResult::warning("Cache", format!("Cannot read cache: {}", e), true),
    }
}

async fn check_skills() -> CheckResult {
    let skills_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield")
        .join("skills");

    if !skills_dir.exists() {
        return CheckResult::warning(
            "Skills",
            format!(
                "Skills dir not found at {}. Will be created.",
                skills_dir.display()
            ),
            true,
        );
    }

    match std::fs::read_dir(&skills_dir) {
        Ok(entries) => {
            let count = entries.filter(|e| e.is_ok()).count();
            CheckResult::healthy("Skills", format!("{} skill entries", count))
        }
        Err(e) => CheckResult::warning("Skills", format!("Cannot read skills: {}", e), false),
    }
}

async fn check_binary() -> CheckResult {
    match std::env::current_exe() {
        Ok(path) => {
            if path.exists() {
                CheckResult::healthy("Binary", format!("Executable at {}", path.display()))
            } else {
                CheckResult::critical("Binary", "Executable path does not exist", false)
            }
        }
        Err(e) => CheckResult::warning("Binary", format!("Cannot detect executable: {}", e), false),
    }
}

async fn check_sessions_dir() -> CheckResult {
    let sessions_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield")
        .join("sessions");

    if !sessions_dir.exists() {
        return CheckResult::warning(
            "Sessions",
            format!(
                "Sessions dir not found at {}. Will be created on first export.",
                sessions_dir.display()
            ),
            true,
        );
    }

    match std::fs::read_dir(&sessions_dir) {
        Ok(entries) => {
            let count = entries
                .filter(|e| {
                    e.as_ref()
                        .map(|entry| {
                            entry.path().extension().and_then(|s| s.to_str()) == Some("json")
                        })
                        .unwrap_or(false)
                })
                .count();
            CheckResult::healthy("Sessions", format!("{} exported sessions", count))
        }
        Err(e) => CheckResult::warning("Sessions", format!("Cannot read sessions: {}", e), false),
    }
}

async fn try_fix(component: &str) -> Result<String> {
    match component {
        "Config" => {
            let config_dir = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshield");
            std::fs::create_dir_all(&config_dir)?;
            let config_path = config_dir.join("config.toml");
            write_default_config(&config_path)
        }
        "Memory DB" => {
            let data_dir = dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshield");
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("memory.db");
            crate::memory::MemoryStore::new(&db_path)
                .context("Failed to initialize memory database")?;
            Ok(format!(
                "Initialized memory database at {}",
                db_path.display()
            ))
        }
        "Cache" => {
            let cache_dir = dirs::cache_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshield");
            std::fs::create_dir_all(&cache_dir)?;
            Ok(format!(
                "Created cache directory at {}",
                cache_dir.display()
            ))
        }
        "Skills" => {
            let skills_dir = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshield")
                .join("skills");
            std::fs::create_dir_all(&skills_dir)?;
            Ok(format!(
                "Created skills directory at {}",
                skills_dir.display()
            ))
        }
        "Sessions" => {
            let sessions_dir = dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshield")
                .join("sessions");
            std::fs::create_dir_all(&sessions_dir)?;
            Ok(format!(
                "Created sessions directory at {}",
                sessions_dir.display()
            ))
        }
        _ => Err(anyhow::anyhow!("No fix available for {}", component)),
    }
}

/// Write `Config::default()` to `config_path`, backing up a non-empty existing
/// file first. The first backup is `config.toml.bak`. If that path already
/// exists, a timestamped `config.toml.bak-<YYYYMMDDThhmmssZ>` is used instead.
/// A non-empty config is never overwritten unless that backup was written.
pub(crate) fn write_default_config(config_path: &std::path::Path) -> Result<String> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let backup = backup_existing_config(config_path)?;
    let default_config = crate::config::Config::default();
    let toml =
        toml::to_string_pretty(&default_config).context("Failed to serialize default config")?;
    std::fs::write(config_path, toml).context("Failed to write default config")?;
    Ok(match backup {
        Some(bak) => format!(
            "Created default config at {} (previous config saved to {})",
            config_path.display(),
            bak.display()
        ),
        None => format!("Created default config at {}", config_path.display()),
    })
}

/// Copy a non-empty existing config to a backup path. Returns `Ok(None)` when
/// the file is missing or empty (those can be replaced without a backup).
/// Fails without touching `config_path` if the backup cannot be written.
pub(crate) fn backup_existing_config(
    config_path: &std::path::Path,
) -> Result<Option<std::path::PathBuf>> {
    if !config_path.exists() {
        return Ok(None);
    }
    let contents =
        std::fs::read(config_path).context("Failed to read existing config for backup")?;
    if contents.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(None);
    }
    let bak = unique_config_backup_path(config_path);
    std::fs::write(&bak, &contents).with_context(|| {
        format!(
            "Failed to write config backup at {}; leaving existing config in place",
            bak.display()
        )
    })?;
    Ok(Some(bak))
}

fn unique_config_backup_path(config_path: &std::path::Path) -> std::path::PathBuf {
    let parent = config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let plain = parent.join("config.toml.bak");
    if !plain.exists() {
        return plain;
    }
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let stamped = parent.join(format!("config.toml.bak-{ts}"));
    if !stamped.exists() {
        return stamped;
    }
    for i in 1..1000 {
        let candidate = parent.join(format!("config.toml.bak-{ts}-{i}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!(
        "config.toml.bak-{ts}-{}",
        uuid::Uuid::new_v4().simple()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openshield-doctor-cfg-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_config_fix_writes_bak_before_overwrite() {
        let dir = scratch();
        let path = dir.join("config.toml");
        let original = "default_model = \"keep-me\"\n";
        std::fs::write(&path, original).unwrap();

        let msg = write_default_config(&path).unwrap();
        let bak = dir.join("config.toml.bak");
        assert!(bak.exists(), "expected config.toml.bak");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), original);
        let rewritten = std::fs::read_to_string(&path).unwrap();
        assert_ne!(rewritten, original);
        assert!(!rewritten.trim().is_empty());
        assert!(msg.contains("config.toml.bak"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_fix_timestamps_bak_if_present() {
        let dir = scratch();
        let path = dir.join("config.toml");
        std::fs::write(dir.join("config.toml.bak"), "old-backup").unwrap();
        let original = "providers = { gone = true }\n";
        std::fs::write(&path, original).unwrap();

        write_default_config(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml.bak")).unwrap(),
            "old-backup"
        );
        let stamped: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("config.toml.bak-"))
            .collect();
        assert_eq!(
            stamped.len(),
            1,
            "expected one timestamped backup, got {stamped:?}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(&stamped[0])).unwrap(),
            original
        );
        let rewritten = std::fs::read_to_string(&path).unwrap();
        assert_ne!(rewritten, original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_fix_skips_backup_when_missing_or_empty() {
        let dir = scratch();
        let path = dir.join("config.toml");
        write_default_config(&path).unwrap();
        assert!(path.exists());
        assert!(!dir.join("config.toml.bak").exists());

        std::fs::write(&path, "  \n").unwrap();
        write_default_config(&path).unwrap();
        assert!(!dir.join("config.toml.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Directory write-bit semantics are reliable on Unix; Windows treats
    // directory readonly differently, so this guarantee is checked here.
    #[cfg(unix)]
    #[test]
    fn test_config_fix_refuses_overwrite_if_backup_fails() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        let path = dir.join("config.toml");
        let original = "do-not-lose-me = true\n";
        std::fs::write(&path, original).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = write_default_config(&path);
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode));
        assert!(result.is_err(), "expected backup failure, got {result:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
