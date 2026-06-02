use super::Tool;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Global undo stack — stores (original_path, backup_path) for last edit
static LAST_BACKUP: std::sync::OnceLock<Arc<Mutex<Option<(PathBuf, PathBuf)>>>> = std::sync::OnceLock::new();

fn get_backup_store() -> Arc<Mutex<Option<(PathBuf, PathBuf)>>> {
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
        "Multi-file editing: read, write, replace, patch. Usage: edit <read|write|replace|patch> <path> [args]"
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

    fn usage(&self) -> String {
        "Edit tool usage:\n\
         edit read <path>                    - Read file with line numbers\n\
         edit write <path> <content>         - Write file (creates dirs)\n\
         edit replace <path> <old> ||| <new> - Replace first occurrence\n\
         edit patch <path> <old> ||| <new>   - Multi-line patch"
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
}
