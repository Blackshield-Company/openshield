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

    pub fn warning(component: impl Into<String>, message: impl Into<String>, fixable: bool) -> Self {
        Self {
            component: component.into(),
            status: CheckStatus::Warning,
            message: message.into(),
            fixable,
        }
    }

    pub fn critical(component: impl Into<String>, message: impl Into<String>, fixable: bool) -> Self {
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
        self.checks.iter().filter(|c| c.status == CheckStatus::Healthy).count()
    }

    pub fn warning_count(&self) -> usize {
        self.checks.iter().filter(|c| c.status == CheckStatus::Warning).count()
    }

    pub fn critical_count(&self) -> usize {
        self.checks.iter().filter(|c| c.status == CheckStatus::Critical).count()
    }

    pub fn print(&self) {
        println!("\n{BOLD}{CYAN}🦈 OpenShark Doctor{RESET}");
        println!("{CYAN}{}{RESET}", "═".repeat(60));

        for check in &self.checks {
            let (icon, status_str, status_color) = match check.status {
                CheckStatus::Healthy => ("✅", "HEALTHY", GREEN),
                CheckStatus::Warning => ("⚠️", "WARNING", YELLOW),
                CheckStatus::Critical => ("❌", "CRITICAL", RED),
            };
            println!(
                "{} {BOLD}{:<20}{RESET} [{status_color}{}{RESET}] {}",
                icon,
                check.component,
                status_str,
                check.message
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
            println!("\n{BOLD}{RED}⚠️  Critical issues found. Run `openshark doctor --fix` to auto-repair.{RESET}");
        } else if self.warning_count() > 0 {
            println!("\n{YELLOW}💡 Warnings found. Run `openshark doctor --fix` to auto-repair.{RESET}");
        } else {
            println!("\n{BOLD}{GREEN}🎉 All systems healthy!{RESET}");
        }
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
            if check.fixable && (check.status == CheckStatus::Warning || check.status == CheckStatus::Critical) {
                if let Ok(fix_msg) = try_fix(&check.component).await {
                    report.fixes_applied.push(fix_msg);
                }
            }
        }
    }

    Ok(report)
}

async fn check_config() -> CheckResult {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshark");
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
                Ok(_) => CheckResult::healthy("Config", format!("Valid TOML at {}", config_path.display())),
                Err(e) => CheckResult::critical("Config", format!("Invalid TOML: {}", e), true),
            }
        }
        Err(e) => CheckResult::critical("Config", format!("Cannot read config: {}", e), false),
    }
}

async fn check_providers() -> CheckResult {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshark");

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
            "No API key env files found. Add keys to ~/.config/openshark/*.env",
            false,
        );
    }

    CheckResult::healthy("Providers", format!("{}/{} API key files present", found, total))
}

async fn check_memory_db() -> CheckResult {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshark");
    let db_path = data_dir.join("memory.db");

    if !db_path.exists() {
        return CheckResult::warning(
            "Memory DB",
            format!("Database not found at {}. Will be created on first use.", db_path.display()),
            true,
        );
    }

    match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            match conn.execute("PRAGMA integrity_check;", []) {
                Ok(_) => CheckResult::healthy("Memory DB", format!("Database OK at {}", db_path.display())),
                Err(e) => CheckResult::critical("Memory DB", format!("Corruption detected: {}", e), true),
            }
        }
        Err(e) => CheckResult::critical("Memory DB", format!("Cannot open: {}", e), true),
    }
}

async fn check_cache() -> CheckResult {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshark");

    if !cache_dir.exists() {
        return CheckResult::warning(
            "Cache",
            format!("Cache dir not found at {}. Will be created.", cache_dir.display()),
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
        .join("openshark")
        .join("skills");

    if !skills_dir.exists() {
        return CheckResult::warning(
            "Skills",
            format!("Skills dir not found at {}. Will be created.", skills_dir.display()),
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
        .join("openshark")
        .join("sessions");

    if !sessions_dir.exists() {
        return CheckResult::warning(
            "Sessions",
            format!("Sessions dir not found at {}. Will be created on first export.", sessions_dir.display()),
            true,
        );
    }

    match std::fs::read_dir(&sessions_dir) {
        Ok(entries) => {
            let count = entries.filter(|e| {
                e.as_ref().map(|entry| {
                    entry.path().extension().and_then(|s| s.to_str()) == Some("json")
                }).unwrap_or(false)
            }).count();
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
                .join("openshark");
            std::fs::create_dir_all(&config_dir)?;
            let default_config = crate::config::Config::default();
            let toml = toml::to_string_pretty(&default_config)
                .context("Failed to serialize default config")?;
            let config_path = config_dir.join("config.toml");
            std::fs::write(&config_path, toml)
                .context("Failed to write default config")?;
            Ok(format!("Created default config at {}", config_path.display()))
        }
        "Memory DB" => {
            let data_dir = dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshark");
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("memory.db");
            let conn = rusqlite::Connection::open(&db_path)
                .context("Failed to create memory database")?;
            conn.execute(
                "CREATE TABLE IF NOT EXISTS messages (
                    id INTEGER PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
                [],
            )?;
            Ok(format!("Recreated memory database at {}", db_path.display()))
        }
        "Cache" => {
            let cache_dir = dirs::cache_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshark");
            std::fs::create_dir_all(&cache_dir)?;
            Ok(format!("Created cache directory at {}", cache_dir.display()))
        }
        "Skills" => {
            let skills_dir = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshark")
                .join("skills");
            std::fs::create_dir_all(&skills_dir)?;
            Ok(format!("Created skills directory at {}", skills_dir.display()))
        }
        "Sessions" => {
            let sessions_dir = dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshark")
                .join("sessions");
            std::fs::create_dir_all(&sessions_dir)?;
            Ok(format!("Created sessions directory at {}", sessions_dir.display()))
        }
        _ => Err(anyhow::anyhow!("No fix available for {}", component)),
    }
}
