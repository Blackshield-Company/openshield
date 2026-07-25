//! Build script: stamps the binary with the current git commit and build date
//! so the TUI splash version line never goes stale.

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // Git short hash (falls back to "unknown" outside a repo)
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    // Build date in YYYY.M.D form (UTC) — no external crates needed
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs / 86_400);

    println!("cargo:rustc-env=OS_GIT_HASH={hash}");
    println!("cargo:rustc-env=OS_BUILD_DATE={y}.{m}.{d}");

    // Rebuild when the checked-out commit changes
    println!("cargo:rerun-if-changed=.git/HEAD");
}

/// Days since Unix epoch -> (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: u64) -> (i64, u32, u32) {
    let z = z as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
