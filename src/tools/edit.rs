use super::Tool;
use anyhow::{Context, Result};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type BackupPair = (PathBuf, PathBuf);

/// Global undo stack — stores (original_path, backup_path) for last edit
static LAST_BACKUP: std::sync::OnceLock<Arc<Mutex<Option<BackupPair>>>> = std::sync::OnceLock::new();

fn get_backup_store() -> Arc<Mutex<Option<BackupPair>>> {
    LAST_BACKUP.get_or_init(|| Arc::new(Mutex::new(None))).clone()
}

/// Perform a backup before editing and store it for potential undo.
fn backup_before_edit(path: &Path) -> Result<PathBuf> {
    let backup_path = path.with_extension("openshark_backup");
    fs::copy(path, &backup_path)
        .with_context(|| format!("Failed to create backup of {}", path.display()))?;
    *get_backup_store().lock().unwrap() = Some((path.to_path_buf(), backup_path.clone()));
    Ok(backup_path)
}

/// Undo the last edit by restoring from backup.
pub fn undo_last_edit() -> Result<String> {
    let store = get_backup_store();
    let mut guard = store.lock().unwrap();
    if let Some((original, backup)) = guard.take() {
        fs::copy(&backup, &original)
            .with_context(|| format!("Failed to restore backup to {}", original.display()))?;
        let _ = fs::remove_file(&backup);
        Ok(format!("Undo successful: restored {}", original.display()))
    } else {
        Ok("Nothing to undo".to_string())
    }
}

pub struct EditTool;

impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Multi-file editing: read, write, replace, patch, diff, rewrite, lines. \
         Usage: edit <read|write|replace|patch|diff|rewrite|lines> <path> [args]"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return Ok(self.usage());
        }

        let cmd = parts[0];
        let rest = parts[1];

        match cmd {
            "read" => self.read_file(rest),
            "write" => self.write_file(rest),
            "replace" => self.replace_in_file(rest),
            "patch" => self.apply_patch(rest),
            "diff" => self.apply_unified_diff(rest),
            "rewrite" => self.rewrite_file(rest),
            "lines" => self.replace_lines(rest),
            _ => Ok(format!("Unknown edit command: {}\n{}", cmd, self.usage())),
        }
    }
}

impl EditTool {
    fn read_file(&self, path: &str) -> Result<String> {
        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;

        // Add line numbers for easier reference
        let numbered: Vec<String> = content
            .lines()
            .enumerate()
            .map(|(i, line)| format!("{:4}| {}", i + 1, line))
            .collect();

        Ok(numbered.join("\n"))
    }

    fn write_file(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return Ok("Usage: edit write <path> <content>".to_string());
        }

        let path = parts[0];
        let content = parts[1];

        // Create parent dirs if needed
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory for {}", path))?;
        }

        // Backup existing file before overwriting
        if Path::new(path).exists() {
            backup_before_edit(Path::new(path))?;
        }

        fs::write(path, content).with_context(|| format!("Failed to write {}", path))?;

        Ok(format!("Written {} bytes to {}", content.len(), path))
    }

    fn replace_in_file(&self, args: &str) -> Result<String> {
        // Format: edit replace <path> <old_string> <new_string>
        // We use a delimiter approach: path, then old|||new
        let delimiter = " ||| ";
        let parts: Vec<&str> = args.splitn(2, delimiter).collect();
        if parts.len() < 2 {
            return Ok(format!(
                "Usage: edit replace <path>{}<old_string>{}<new_string>",
                delimiter, delimiter
            ));
        }

        let path_part = parts[0];
        let path_parts: Vec<&str> = path_part.splitn(2, ' ').collect();
        if path_parts.len() < 2 {
            return Ok("Usage: edit replace <path> <old_string> ||| <new_string>".to_string());
        }

        let path = path_parts[0];
        let old_str = path_parts[1];
        let new_str = parts[1];

        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;

        if !content.contains(old_str) {
            return Ok(format!(
                "String not found in {}. Use 'edit read' to see exact content.",
                path
            ));
        }

        backup_before_edit(Path::new(path))?;

        let new_content = content.replacen(old_str, new_str, 1);
        fs::write(path, new_content).with_context(|| format!("Failed to write {}", path))?;

        Ok(format!("Replaced in {}", path))
    }

    fn apply_patch(&self, args: &str) -> Result<String> {
        // Format: edit patch <path> <old_lines> ||| <new_lines>
        let delimiter = " ||| ";
        let parts: Vec<&str> = args.splitn(2, delimiter).collect();
        if parts.len() < 2 {
            return Ok(format!(
                "Usage: edit patch <path>{}<old_lines>{}<new_lines>",
                delimiter, delimiter
            ));
        }

        let path_part = parts[0];
        let path_parts: Vec<&str> = path_part.splitn(2, ' ').collect();
        if path_parts.len() < 2 {
            return Ok("Usage: edit patch <path> <old_lines> ||| <new_lines>".to_string());
        }

        let path = path_parts[0];
        let old_lines = path_parts[1];
        let new_lines = parts[1];

        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;

        if !content.contains(old_lines) {
            return Ok(format!(
                "Patch context not found in {}. Content may have changed.",
                path
            ));
        }

        backup_before_edit(Path::new(path))?;

        let new_content = content.replacen(old_lines, new_lines, 1);
        fs::write(path, new_content).with_context(|| format!("Failed to write {}", path))?;

        Ok(format!("Patched {}", path))
    }

    /// Apply a unified diff (git diff format) to a file.
    ///
    /// Format: edit diff <path>
    /// --- <content with unified diff markers>
    ///
    /// The diff must be a unified diff with @@ -start,count +start,count @@ headers.
    fn apply_unified_diff(&self, args: &str) -> Result<String> {
        // First line is path, rest is the diff content
        let mut lines = args.lines();
        let path = lines.next().unwrap_or("").trim();
        if path.is_empty() {
            return Ok("Usage: edit diff <path>\n<unified diff content>".to_string());
        }

        let diff_content: String = lines.collect::<Vec<_>>().join("\n");
        if diff_content.trim().is_empty() {
            return Ok("No diff content provided.".to_string());
        }

        let file_content =
            fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;

        let file_lines: Vec<&str> = file_content.lines().collect();
        let mut result_lines: Vec<String> = file_lines.iter().map(|s| s.to_string()).collect();

        // Parse unified diff hunks: @@ -start,count +start,count @@
        let hunk_re = Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@").unwrap();

        let diff_lines: Vec<&str> = diff_content.lines().collect();
        let mut i = 0;
        let mut hunk_count = 0;

        // We process hunks in reverse order so line numbers stay valid
        let mut hunks: Vec<(usize, usize, Vec<&str>)> = Vec::new();

        while i < diff_lines.len() {
            let line = diff_lines[i];
            if let Some(cap) = hunk_re.captures(line) {
                let old_start: usize = cap[1].parse().unwrap_or(1);
                let _old_count: usize = cap
                    .get(2)
                    .map(|m| m.as_str().parse().unwrap_or(0))
                    .unwrap_or(0);
                let new_start: usize = cap[3].parse().unwrap_or(1);
                let _new_count: usize = cap
                    .get(4)
                    .map(|m| m.as_str().parse().unwrap_or(0))
                    .unwrap_or(0);

                // Collect hunk body lines
                let mut hunk_body: Vec<&str> = Vec::new();
                i += 1;
                while i < diff_lines.len() && !diff_lines[i].starts_with("@@") {
                    hunk_body.push(diff_lines[i]);
                    i += 1;
                }

                hunks.push((old_start, new_start, hunk_body));
                hunk_count += 1;
            } else {
                i += 1;
            }
        }

        if hunk_count == 0 {
            return Ok(
                "No valid diff hunks found. Expected @@ -start,count +start,count @@ headers."
                    .to_string(),
            );
        }

        // Process hunks in reverse order (by old_start descending) so line numbers don't shift
        hunks.sort_by_key(|b| std::cmp::Reverse(b.0));

        for (old_start, _new_start, hunk_body) in hunks {
            let start_idx = old_start.saturating_sub(1); // 1-based to 0-based
            let mut offset: isize = 0;

            for hunk_line in &hunk_body {
                let idx = (start_idx as isize + offset) as usize;
                if hunk_line.starts_with('-') {
                    // Remove line at current position
                    if idx < result_lines.len() {
                        result_lines.remove(idx);
                    }
                    // offset stays same since we removed current line
                } else if let Some(rest) = hunk_line.strip_prefix('+') {
                    // Insert after current position
                    result_lines.insert(idx, rest.to_string());
                    offset += 1; // we added a line, so advance
                } else if hunk_line.starts_with(' ') {
                    // Context line — just advance
                    offset += 1;
                }
            }
        }

        backup_before_edit(Path::new(path))?;

        let new_content = result_lines.join("\n");
        fs::write(path, new_content).with_context(|| format!("Failed to write {}", path))?;

        Ok(format!("Applied diff to {} ({} hunks)", path, hunk_count))
    }

    /// Rewrite an entire file with new content.
    ///
    /// Format: edit rewrite <path>
    /// ---
    /// <new content>
    fn rewrite_file(&self, args: &str) -> Result<String> {
        // First line is path, rest is content
        let mut lines = args.lines();
        let path = lines.next().unwrap_or("").trim();
        if path.is_empty() {
            return Ok("Usage: edit rewrite <path>\n<new content>".to_string());
        }

        let content: String = lines.collect::<Vec<_>>().join("\n");

        // Create parent dirs if needed
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory for {}", path))?;
        }

        // Backup existing file
        if Path::new(path).exists() {
            backup_before_edit(Path::new(path))?;
        }

        fs::write(path, &content).with_context(|| format!("Failed to write {}", path))?;

        Ok(format!(
            "Rewrote {} ({} bytes, {} lines)",
            path,
            content.len(),
            content.lines().count()
        ))
    }

    /// Replace a range of lines by line numbers.
    ///
    /// Format: edit lines <path> <start>-<end> ||| <new_lines>
    /// Replaces lines start through end (1-based, inclusive) with new_lines.
    fn replace_lines(&self, args: &str) -> Result<String> {
        let delimiter = " ||| ";
        let parts: Vec<&str> = args.splitn(2, delimiter).collect();
        if parts.len() < 2 {
            return Ok(format!(
                "Usage: edit lines <path> <start>-<end>{}<new_lines>",
                delimiter
            ));
        }

        let path_range = parts[0].trim();
        let new_lines = parts[1];

        // Parse path and range
        let space_idx = path_range.rfind(' ').unwrap_or(0);
        let path = &path_range[..space_idx].trim();
        let range_str = &path_range[space_idx..].trim();

        if path.is_empty() || range_str.is_empty() {
            return Ok("Usage: edit lines <path> <start>-<end> ||| <new_lines>".to_string());
        }

        let range_parts: Vec<&str> = range_str.split('-').collect();
        if range_parts.len() != 2 {
            return Ok("Range must be <start>-<end> (e.g., 5-10)".to_string());
        }

        let start: usize = range_parts[0].parse().unwrap_or(0);
        let end: usize = range_parts[1].parse().unwrap_or(0);

        if start == 0 || end == 0 || start > end {
            return Ok("Invalid line range. Must be 1-based and start <= end.".to_string());
        }

        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;
        let mut file_lines: Vec<&str> = content.lines().collect();

        let start_idx = start.saturating_sub(1);
        let end_idx = end.saturating_sub(1);

        if start_idx >= file_lines.len() {
            return Ok(format!(
                "Start line {} is beyond file length ({} lines)",
                start,
                file_lines.len()
            ));
        }

        backup_before_edit(Path::new(path))?;

        // Replace the range
        let new_lines_vec: Vec<&str> = new_lines.lines().collect();
        file_lines.splice(start_idx..=end_idx.min(file_lines.len() - 1), new_lines_vec);

        let new_content = file_lines.join("\n");
        fs::write(path, new_content).with_context(|| format!("Failed to write {}", path))?;

        Ok(format!(
            "Replaced lines {}-{} in {} (now {} lines)",
            start,
            end,
            path,
            file_lines.len()
        ))
    }

    fn usage(&self) -> String {
        "Edit tool usage:\n\
         edit read <path>                              - Read file with line numbers\n\
         edit write <path> <content>                   - Write file (creates dirs)\n\
         edit replace <path> <old> ||| <new>           - Replace first occurrence\n\
         edit patch <path> <old> ||| <new>             - Multi-line patch\n\
         edit diff <path>\n<unified diff>             - Apply unified diff\n\
         edit rewrite <path>\n<new content>            - Rewrite entire file\n\
         edit lines <path> <start>-<end> ||| <new>     - Replace line range"
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = format!("/tmp/openshark_edit_test_{}_{}", std::process::id(), count);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &str) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_read_file() {
        let dir = temp_dir();
        let path = format!("{}/test_read.txt", dir);
        fs::write(&path, "line1\nline2\nline3").unwrap();

        let tool = EditTool;
        let result = tool.execute(&format!("read {}", path)).unwrap();

        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
        cleanup(&dir);
    }

    #[test]
    fn test_write_file() {
        let dir = temp_dir();
        let path = format!("{}/test_write.txt", dir);

        let tool = EditTool;
        let result = tool
            .execute(&format!("write {} Hello World", path))
            .unwrap();

        assert!(result.contains("Written"));
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "Hello World");
        cleanup(&dir);
    }

    #[test]
    fn test_write_file_creates_dirs() {
        let dir = temp_dir();
        let path = format!("{}/subdir/nested/test.txt", dir);

        let tool = EditTool;
        let result = tool.execute(&format!("write {} content", path)).unwrap();

        assert!(result.contains("Written"));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("content"));
        cleanup(&dir);
    }

    #[test]
    fn test_replace_in_file() {
        let dir = temp_dir();
        let path = format!("{}/test_replace.txt", dir);
        fs::write(&path, "Hello old world").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(&format!("replace {} old ||| new", path))
            .unwrap();

        assert!(result.contains("Replaced"));
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "Hello new world");
        cleanup(&dir);
    }

    #[test]
    fn test_replace_string_not_found() {
        let dir = temp_dir();
        let path = format!("{}/test_replace_notfound.txt", dir);
        fs::write(&path, "Hello world").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(&format!("replace {} nonexistent ||| new", path))
            .unwrap();

        assert!(result.contains("not found"));
        cleanup(&dir);
    }

    #[test]
    fn test_patch_file() {
        let dir = temp_dir();
        let path = format!("{}/test_patch.txt", dir);
        fs::write(&path, "line1\nline2\nline3").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(&format!(
                "patch {} line2\nline3 ||| line2_new\nline3_new",
                path
            ))
            .unwrap();

        assert!(result.contains("Patched"));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("line2_new"));
        cleanup(&dir);
    }

    #[test]
    fn test_patch_context_not_found() {
        let dir = temp_dir();
        let path = format!("{}/test_patch_notfound.txt", dir);
        fs::write(&path, "Hello world").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(&format!("patch {} nonexistent ||| new", path))
            .unwrap();

        assert!(result.contains("not found"));
        cleanup(&dir);
    }

    #[test]
    fn test_unknown_command() {
        let tool = EditTool;
        let result = tool.execute("unknown something").unwrap();
        assert!(result.contains("Unknown edit command"));
    }

    #[test]
    fn test_empty_args() {
        let tool = EditTool;
        let result = tool.execute("").unwrap();
        assert!(result.contains("Edit tool usage"));
    }

    #[test]
    fn test_rewrite_file() {
        let dir = temp_dir();
        let path = format!("{}/test_rewrite.txt", dir);
        fs::write(&path, "old content").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(&format!("rewrite {}\nnew line 1\nnew line 2", path))
            .unwrap();

        assert!(result.contains("Rewrote"));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("new line 1"));
        assert!(content.contains("new line 2"));
        assert!(!content.contains("old content"));
        cleanup(&dir);
    }

    #[test]
    fn test_rewrite_file_creates_new() {
        let dir = temp_dir();
        let path = format!("{}/new_file.txt", dir);

        let tool = EditTool;
        let result = tool.execute(&format!("rewrite {}\nbrand new content", path)).unwrap();

        assert!(result.contains("Rewrote"));
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "brand new content");
        cleanup(&dir);
    }

    #[test]
    fn test_replace_lines() {
        let dir = temp_dir();
        let path = format!("{}/test_lines.txt", dir);
        fs::write(&path, "line1\nline2\nline3\nline4\nline5").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(&format!("lines {} 2-4 ||| new2\nnew3\nnew4", path))
            .unwrap();

        assert!(result.contains("Replaced lines 2-4"));
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "new2");
        assert_eq!(lines[2], "new3");
        assert_eq!(lines[3], "new4");
        assert_eq!(lines[4], "line5");
        cleanup(&dir);
    }

    #[test]
    fn test_replace_lines_invalid_range() {
        let dir = temp_dir();
        let path = format!("{}/test_lines_invalid.txt", dir);
        fs::write(&path, "line1\nline2").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(&format!("lines {} 5-10 ||| new", path))
            .unwrap();

        assert!(result.contains("beyond file length"));
        cleanup(&dir);
    }

    #[test]
    fn test_apply_unified_diff() {
        let dir = temp_dir();
        let path = format!("{}/test_diff.txt", dir);
        fs::write(&path, "line1\nline2\nline3\nline4\nline5").unwrap();

        let diff = format!(
            "{}\n@@ -2,3 +2,3 @@\n-line2\n+line2_modified\n line3\n-line4\n+line4_modified",
            path
        );

        let tool = EditTool;
        let result = tool.execute(&format!("diff {}", diff)).unwrap();

        assert!(result.contains("Applied diff"));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("line2_modified"));
        assert!(content.contains("line4_modified"));
        assert!(content.contains("line1"));
        assert!(content.contains("line3"));
        assert!(content.contains("line5"));
        cleanup(&dir);
    }

    #[test]
    fn test_apply_unified_diff_no_hunks() {
        let dir = temp_dir();
        let path = format!("{}/test_diff_nohunks.txt", dir);
        fs::write(&path, "some content").unwrap();

        let tool = EditTool;
        let result = tool
            .execute(&format!("diff {}\nnot a valid diff", path))
            .unwrap();

        assert!(result.contains("No valid diff hunks"));
        cleanup(&dir);
    }
}
