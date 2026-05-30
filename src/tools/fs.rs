use anyhow::{Context, Result};
use std::fs;
use super::Tool;

pub struct FsTool;

impl Tool for FsTool {
    fn name(&self) -> &str {
        "fs"
    }

    fn description(&self) -> &str {
        "Read, write, and search files"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return Ok("Usage: fs <read|write|list> <path> [content]".to_string());
        }

        let cmd = parts[0];
        let path_str = parts[1];

        match cmd {
            "read" => {
                let content = fs::read_to_string(path_str)
                    .with_context(|| format!("Failed to read {}", path_str))?;
                Ok(content)
            }
            "write" => {
                let write_parts: Vec<&str> = path_str.splitn(2, ' ').collect();
                if write_parts.len() < 2 {
                    return Ok("Usage: fs write <path> <content>".to_string());
                }
                fs::write(write_parts[0], write_parts[1])
                    .with_context(|| format!("Failed to write {}", write_parts[0]))?;
                Ok("Written successfully".to_string())
            }
            "list" => {
                let entries = fs::read_dir(path_str)
                    .with_context(|| format!("Failed to list {}", path_str))?;
                let mut result = String::new();
                for entry in entries {
                    let entry = entry?;
                    let name = entry.file_name();
                    result.push_str(&format!("{}\n", name.to_string_lossy()));
                }
                Ok(result)
            }
            _ => Ok(format!("Unknown fs command: {}", cmd)),
        }
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
    fn test_fs_write() {
        let dir = temp_dir();
        let path = format!("{}/test_write.txt", dir);

        let tool = FsTool;
        let result = tool.execute(&format!("write {} Hello World", path)).unwrap();

        assert_eq!(result, "Written successfully");
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
    fn test_fs_unknown_command() {
        let tool = FsTool;
        let result = tool.execute("unknown /tmp").unwrap();
        assert!(result.contains("Unknown fs command"));
    }

    #[test]
    fn test_fs_empty_args() {
        let tool = FsTool;
        let result = tool.execute("").unwrap();
        assert!(result.contains("Usage"));
    }
}
