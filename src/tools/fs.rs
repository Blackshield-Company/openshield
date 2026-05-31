use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;
use super::Tool;

pub struct FsTool;

impl Tool for FsTool {
    fn name(&self) -> &str {
        "fs"
    }

    fn description(&self) -> &str {
        "Filesystem operations: read, write, list, tree, stat, glob, find, cat"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return Ok(USAGE.to_string());
        }

        let cmd = parts[0];
        let rest = parts[1];

        match cmd {
            "read" => cmd_read(rest),
            "write" => cmd_write(rest),
            "list" => cmd_list(rest),
            "tree" => cmd_tree(rest),
            "stat" => cmd_stat(rest),
            "glob" => cmd_glob(rest),
            "find" => cmd_find(rest),
            "cat" => cmd_cat(rest),
            _ => Ok(format!("Unknown fs command: {}\n{}", cmd, USAGE)),
        }
    }
}

const USAGE: &str = r#"Filesystem tool usage:
  fs read <path>                    — Read entire file
  fs cat <path> [offset] [limit]    — Read file with pagination
  fs write <path> <content>         — Write content to file
  fs list <path>                    — List directory entries
  fs tree <path> [depth]            — Recursive directory tree (default depth 3)
  fs stat <path>                    — File/directory metadata
  fs glob <pattern>                 — Find files matching glob pattern
  fs find <path> <name>             — Find files by name under path
"#;

fn expand_path(path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(path).to_string())
}

fn cmd_read(path_str: &str) -> Result<String> {
    let path = expand_path(path_str);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(content)
}

fn cmd_cat(args: &str) -> Result<String> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    if tokens.is_empty() {
        return Ok("Usage: fs cat <path> [offset] [limit]".to_string());
    }
    let path = expand_path(tokens[0]);
    let offset: usize = tokens.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let limit: usize = tokens.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let lines: Vec<&str> = content.lines().collect();
    let start = offset.min(lines.len());
    let end = (offset + limit).min(lines.len());
    let selected = &lines[start..end];

    let mut result = format!(
        "--- {} (lines {}-{} of {}) ---\n",
        path.display(),
        start,
        end,
        lines.len()
    );
    for (i, line) in selected.iter().enumerate() {
        result.push_str(&format!("{:4} | {}\n", start + i + 1, line));
    }
    if end < lines.len() {
        result.push_str(&format!(
            "\n... {} more lines. Use: fs cat {} {} {}\n",
            lines.len() - end,
            tokens[0],
            end,
            limit
        ));
    }
    Ok(result)
}

fn cmd_write(args: &str) -> Result<String> {
    let write_parts: Vec<&str> = args.splitn(2, ' ').collect();
    if write_parts.len() < 2 {
        return Ok("Usage: fs write <path> <content>".to_string());
    }
    let path = expand_path(write_parts[0]);
    fs::write(&path, write_parts[1])
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(format!("Written successfully to {}", path.display()))
}

fn cmd_list(path_str: &str) -> Result<String> {
    let path = expand_path(path_str);
    let entries = fs::read_dir(&path)
        .with_context(|| format!("Failed to list {}", path.display()))?;

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata()?;
        let size = if meta.is_file() {
            format_size(meta.len())
        } else {
            "DIR".to_string()
        };
        let modified = meta.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| format_time(d.as_secs()))
            .unwrap_or_else(|| "?".to_string());

        let line = format!("{:>10}  {:>12}  {}", modified, size, name);
        if meta.is_dir() {
            dirs.push(line);
        } else {
            files.push(line);
        }
    }

    dirs.sort();
    files.sort();

    let mut result = format!("Directory: {}\n", path.display());
    result.push_str("MODIFIED       SIZE         NAME\n");
    result.push_str("────────────────────────────────\n");
    for d in &dirs {
        result.push_str(&format!("{}\n", d));
    }
    for f in &files {
        result.push_str(&format!("{}\n", f));
    }
    result.push_str(&format!("\n{} dirs, {} files\n", dirs.len(), files.len()));
    Ok(result)
}

fn cmd_tree(args: &str) -> Result<String> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let path = expand_path(tokens.first().unwrap_or(&"."));
    let max_depth: usize = tokens.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);

    let mut result = format!("{}\n", path.display());
    let mut file_count = 0;
    let mut dir_count = 0;

    for entry in WalkDir::new(&path)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(|e| e.ok())
        .skip(1)
    {
        let depth = entry.depth();
        let indent = "  ".repeat(depth - 1);
        let name = entry.file_name().to_string_lossy();
        let prefix = if depth > 1 { "└─ " } else { "├─ " };

        if entry.file_type().is_dir() {
            result.push_str(&format!("{}{}{}/\n", indent, prefix, name));
            dir_count += 1;
        } else {
            let size = entry.metadata().map(|m| format_size(m.len())).unwrap_or_default();
            result.push_str(&format!("{}{}{}  {}\n", indent, prefix, name, size));
            file_count += 1;
        }
    }

    result.push_str(&format!("\n{} dirs, {} files (depth ≤ {})\n", dir_count, file_count, max_depth));
    Ok(result)
}

fn cmd_stat(path_str: &str) -> Result<String> {
    let path = expand_path(path_str);
    let meta = fs::metadata(&path)
        .with_context(|| format!("Failed to stat {}", path.display()))?;

    let mut result = format!("Path: {}\n", path.display());
    result.push_str(&format!("Type: {}\n", if meta.is_dir() { "directory" } else { "file" }));
    result.push_str(&format!("Size: {} ({} bytes)\n", format_size(meta.len()), meta.len()));

    if let Ok(modified) = meta.modified() {
        if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
            result.push_str(&format!("Modified: {}\n", format_time(dur.as_secs())));
        }
    }
    if let Ok(created) = meta.created() {
        if let Ok(dur) = created.duration_since(std::time::UNIX_EPOCH) {
            result.push_str(&format!("Created:  {}\n", format_time(dur.as_secs())));
        }
    }
    if let Ok(accessed) = meta.accessed() {
        if let Ok(dur) = accessed.duration_since(std::time::UNIX_EPOCH) {
            result.push_str(&format!("Accessed: {}\n", format_time(dur.as_secs())));
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
        result.push_str(&format!("Permissions: {:o}\n", mode & 0o777));
    }

    Ok(result)
}

fn cmd_glob(pattern: &str) -> Result<String> {
    let expanded = shellexpand::tilde(pattern).to_string();
    let mut results = Vec::new();

    // Simple glob: split into base dir and pattern
    let (base, pat) = if expanded.contains('/') {
        let last_slash = expanded.rfind('/').unwrap();
        let base = &expanded[..last_slash + 1];
        let pat = &expanded[last_slash + 1..];
        (base.to_string(), pat.to_string())
    } else {
        (".".to_string(), expanded.clone())
    };

    let base_path = expand_path(&base);
    let regex_pat = glob_to_regex(&pat);
    let re = regex::Regex::new(&regex_pat)?;

    for entry in WalkDir::new(&base_path)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let name = entry.file_name().to_string_lossy();
        if re.is_match(&name) {
            results.push(entry.path().display().to_string());
        }
    }

    results.sort();
    let mut result = format!("Glob: {}\nFound {} matches:\n", expanded, results.len());
    for r in &results {
        result.push_str(&format!("  {}\n", r));
    }
    Ok(result)
}

fn cmd_find(args: &str) -> Result<String> {
    let tokens: Vec<&str> = args.splitn(2, ' ').collect();
    if tokens.len() < 2 {
        return Ok("Usage: fs find <path> <name>".to_string());
    }
    let path = expand_path(tokens[0]);
    let name = tokens[1];

    let mut results = Vec::new();
    for entry in WalkDir::new(&path)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let entry_name = entry.file_name().to_string_lossy();
        if entry_name.contains(name) {
            results.push(entry.path().display().to_string());
        }
    }

    results.sort();
    let mut result = format!("Find '{}' under {}\nFound {} matches:\n", name, path.display(), results.len());
    for r in &results {
        result.push_str(&format!("  {}\n", r));
    }
    Ok(result)
}

fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    format!("{:.1} {}", size, UNITS[unit_idx])
}

fn format_time(unix_secs: u64) -> String {
    let dt = chrono::DateTime::from_timestamp(unix_secs as i64, 0)
        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '.' => regex.push_str("\\."),
            '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }
    regex.push('$');
    regex
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = format!("/tmp/openshark_fs_test_{}_{}", std::process::id(), count);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &str) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_fs_read() {
        let dir = temp_dir();
        let path = format!("{}/test_read.txt", dir);
        fs::write(&path, "Hello, World!").unwrap();

        let tool = FsTool;
        let result = tool.execute(&format!("read {}", path)).unwrap();

        assert_eq!(result, "Hello, World!");
        cleanup(&dir);
    }

    #[test]
    fn test_fs_cat() {
        let dir = temp_dir();
        let path = format!("{}/test_cat.txt", dir);
        let lines: Vec<String> = (1..=20).map(|i| format!("line {}", i)).collect();
        fs::write(&path, lines.join("\n")).unwrap();

        let tool = FsTool;
        let result = tool.execute(&format!("cat {} 5 5", path)).unwrap();

        assert!(result.contains("line 6"));
        assert!(result.contains("line 10"));
        assert!(!result.contains("line 5"));
        cleanup(&dir);
    }

    #[test]
    fn test_fs_write() {
        let dir = temp_dir();
        let path = format!("{}/test_write.txt", dir);

        let tool = FsTool;
        let result = tool.execute(&format!("write {} Hello World", path)).unwrap();

        assert!(result.contains("Written successfully"));
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "Hello World");
        cleanup(&dir);
    }

    #[test]
    fn test_fs_list() {
        let dir = temp_dir();
        fs::write(format!("{}/file1.txt", dir), "").unwrap();
        fs::write(format!("{}/file2.txt", dir), "").unwrap();

        let tool = FsTool;
        let result = tool.execute(&format!("list {}", dir)).unwrap();

        assert!(result.contains("file1.txt"));
        assert!(result.contains("file2.txt"));
        cleanup(&dir);
    }

    #[test]
    fn test_fs_tree() {
        let dir = temp_dir();
        fs::create_dir(format!("{}/subdir", dir)).unwrap();
        fs::write(format!("{}/subdir/nested.txt", dir), "").unwrap();
        fs::write(format!("{}/root.txt", dir), "").unwrap();

        let tool = FsTool;
        let result = tool.execute(&format!("tree {}", dir)).unwrap();

        assert!(result.contains("subdir/"));
        assert!(result.contains("nested.txt"));
        assert!(result.contains("root.txt"));
        cleanup(&dir);
    }

    #[test]
    fn test_fs_stat() {
        let dir = temp_dir();
        let path = format!("{}/test_stat.txt", dir);
        fs::write(&path, "test content").unwrap();

        let tool = FsTool;
        let result = tool.execute(&format!("stat {}", path)).unwrap();

        assert!(result.contains("Type: file"));
        assert!(result.contains("Size:"));
        cleanup(&dir);
    }

    #[test]
    fn test_fs_find() {
        let dir = temp_dir();
        fs::write(format!("{}/foo.txt", dir), "").unwrap();
        fs::write(format!("{}/bar.txt", dir), "").unwrap();

        let tool = FsTool;
        let result = tool.execute(&format!("find {} foo", dir)).unwrap();

        assert!(result.contains("foo.txt"));
        assert!(!result.contains("bar.txt"));
        cleanup(&dir);
    }

    #[test]
    fn test_fs_glob() {
        let dir = temp_dir();
        fs::write(format!("{}/test.txt", dir), "").unwrap();
        fs::write(format!("{}/test.md", dir), "").unwrap();

        let tool = FsTool;
        let result = tool.execute(&format!("glob {}/test.*", dir)).unwrap();

        assert!(result.contains("test.txt"));
        assert!(result.contains("test.md"));
        cleanup(&dir);
    }

    #[test]
    fn test_fs_unknown_command() {
        let tool = FsTool;
        let result = tool.execute("unknown /tmp").unwrap();
        assert!(result.contains("Unknown fs command"));
    }

    #[test]
    fn test_fs_empty_args() {
        let tool = FsTool;
        let result = tool.execute("").unwrap();
        assert!(result.contains("Filesystem") || result.contains("filesystem"));
    }
}
