use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Detect code blocks in a message and extract them.
/// Returns Vec<(language, code)>.
pub fn extract_code_blocks(content: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current_lang = String::new();
    let mut current_code = String::new();

    for line in content.lines() {
        if line.starts_with("```") {
            if in_block {
                // End of block
                blocks.push((current_lang.clone(), current_code.trim().to_string()));
                current_code.clear();
                current_lang.clear();
                in_block = false;
            } else {
                // Start of block
                current_lang = line[3..].trim().to_string();
                in_block = true;
            }
        } else if in_block {
            current_code.push_str(line);
            current_code.push('\n');
        }
    }

    blocks
}

/// Execute code in a sandboxed environment.
/// Returns (stdout, stderr, success).
pub fn execute_code(lang: &str, code: &str) -> Result<(String, String, bool)> {
    let tmp_dir = std::env::temp_dir().join("openshark_sandbox");
    std::fs::create_dir_all(&tmp_dir)?;

    match lang {
        "rust" | "rs" => execute_rust(code, &tmp_dir),
        "python" | "py" => execute_python(code, &tmp_dir),
        "bash" | "sh" | "shell" => execute_bash(code, &tmp_dir),
        "javascript" | "js" | "node" => execute_javascript(code, &tmp_dir),
        _ => {
            // Try to detect shebang or fallback to bash
            if code.starts_with("#!/") {
                execute_shebang(code, &tmp_dir)
            } else {
                execute_bash(code, &tmp_dir)
            }
        }
    }
}

fn execute_rust(code: &str, tmp_dir: &Path) -> Result<(String, String, bool)> {
    let file_path = tmp_dir.join("sandbox.rs");
    let full_code = if code.contains("fn main") {
        code.to_string()
    } else {
        format!("fn main() {{\n{}\n}}", code)
    };
    std::fs::write(&file_path, full_code)?;

    let output = Command::new("timeout")
        .args(["10", "rustc", "--edition", "2021", "-o"])
        .arg(tmp_dir.join("sandbox"))
        .arg(&file_path)
        .output()?;

    if !output.status.success() {
        return Ok((
            String::new(),
            String::from_utf8_lossy(&output.stderr).to_string(),
            false,
        ));
    }

    let run_output = Command::new("timeout")
        .args(["5", tmp_dir.join("sandbox").to_str().unwrap()])
        .output()?;

    Ok((
        String::from_utf8_lossy(&run_output.stdout).to_string(),
        String::from_utf8_lossy(&run_output.stderr).to_string(),
        run_output.status.success(),
    ))
}

fn execute_python(code: &str, tmp_dir: &Path) -> Result<(String, String, bool)> {
    let file_path = tmp_dir.join("sandbox.py");
    std::fs::write(&file_path, code)?;

    let output = Command::new("timeout")
        .args(["10", "python3"])
        .arg(&file_path)
        .output()?;

    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    ))
}

fn execute_bash(code: &str, tmp_dir: &Path) -> Result<(String, String, bool)> {
    let file_path = tmp_dir.join("sandbox.sh");
    std::fs::write(&file_path, code)?;

    let output = Command::new("timeout")
        .args(["10", "bash"])
        .arg(&file_path)
        .output()?;

    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    ))
}

fn execute_javascript(code: &str, tmp_dir: &Path) -> Result<(String, String, bool)> {
    let file_path = tmp_dir.join("sandbox.js");
    std::fs::write(&file_path, code)?;

    let output = Command::new("timeout")
        .args(["10", "node"])
        .arg(&file_path)
        .output()?;

    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    ))
}

fn execute_shebang(code: &str, tmp_dir: &Path) -> Result<(String, String, bool)> {
    let file_path = tmp_dir.join("sandbox_script");
    std::fs::write(&file_path, code)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&file_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&file_path, perms)?;
    }

    let output = Command::new("timeout")
        .args(["10"])
        .arg(&file_path)
        .output()?;

    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    ))
}

/// Run all code blocks from a message and return formatted results.
pub fn run_code_blocks(content: &str) -> Vec<(String, String, String, bool)> {
    let blocks = extract_code_blocks(content);
    let mut results = Vec::new();

    for (lang, code) in blocks {
        match execute_code(&lang, &code) {
            Ok((stdout, stderr, success)) => {
                results.push((lang, stdout, stderr, success));
            }
            Err(e) => {
                results.push((lang, String::new(), format!("Execution error: {}", e), false));
            }
        }
    }

    results
}
