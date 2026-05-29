use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use super::Tool;

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
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path))?;

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

        fs::write(path, content)
            .with_context(|| format!("Failed to write {}", path))?;

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

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path))?;

        if !content.contains(old_str) {
            return Ok(format!(
                "String not found in {}. Use 'edit read' to see exact content.",
                path
            ));
        }

        let new_content = content.replacen(old_str, new_str, 1);
        fs::write(path, new_content)
            .with_context(|| format!("Failed to write {}", path))?;

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

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path))?;

        if !content.contains(old_lines) {
            return Ok(format!(
                "Patch context not found in {}. Content may have changed.",
                path
            ));
        }

        let new_content = content.replacen(old_lines, new_lines, 1);
        fs::write(path, new_content)
            .with_context(|| format!("Failed to write {}", path))?;

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
