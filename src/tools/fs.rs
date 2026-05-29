use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
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
