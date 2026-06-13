/// Simple inline diff generator for file edits.
/// Produces a unified-diff-style output without external dependencies.
///
/// Generate a diff between old and new content.
pub fn generate_diff(old: &str, new: &str, path: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut result = format!("--- {}\n+++ {}\n", path, path);

    // Simple line-by-line diff
    let max_len = old_lines.len().max(new_lines.len());
    let mut i = 0;
    let mut in_hunk = false;

    while i < max_len {
        let old_line = old_lines.get(i);
        let new_line = new_lines.get(i);

        if old_line != new_line {
            if !in_hunk {
                // Start a new hunk
                let start = i.saturating_sub(2);
                let old_count = old_lines.len().saturating_sub(start).min(7);
                let new_count = new_lines.len().saturating_sub(start).min(7);
                result.push_str(&format!(
                    "@@ -{},{} +{},{} @@\n",
                    start + 1,
                    old_count,
                    start + 1,
                    new_count
                ));
                in_hunk = true;

                // Show context lines before change
                for j in start..i {
                    if let Some(line) = old_lines.get(j) {
                        result.push_str(&format!(" {}\n", line));
                    }
                }
            }

            if let Some(old) = old_line {
                if !new_lines.contains(old) {
                    result.push_str(&format!("-{}", old));
                    if !old.ends_with('\n') {
                        result.push('\n');
                    }
                } else {
                    result.push_str(&format!(" {}\n", old));
                }
            }

            if let Some(new) = new_line
                && !old_lines.contains(new)
            {
                result.push_str(&format!("+{}", new));
                if !new.ends_with('\n') {
                    result.push('\n');
                }
            }
        } else if in_hunk {
            // Context line within hunk
            if let Some(line) = old_line {
                result.push_str(&format!(" {}\n", line));
            }
        }

        i += 1;
    }

    if result.ends_with('\n') {
        result.pop();
    }

    result
}

/// Generate a diff preview for a replace operation.
pub fn preview_replace(path: &str, old_str: &str, new_str: &str) -> Result<String, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {}", path, e))?;

    if !content.contains(old_str) {
        return Err(format!("String not found in {}", path));
    }

    let new_content = content.replacen(old_str, new_str, 1);
    Ok(generate_diff(&content, &new_content, path))
}

/// Generate a diff preview for a patch operation.
pub fn preview_patch(path: &str, old_lines: &str, new_lines: &str) -> Result<String, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {}", path, e))?;

    if !content.contains(old_lines) {
        return Err(format!("Patch context not found in {}", path));
    }

    let new_content = content.replacen(old_lines, new_lines, 1);
    Ok(generate_diff(&content, &new_content, path))
}

/// Generate a diff preview for a write operation.
pub fn preview_write(path: &str, new_content: &str) -> Result<String, String> {
    let old_content = std::fs::read_to_string(path).unwrap_or_default();
    Ok(generate_diff(&old_content, new_content, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_basic() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline2_modified\nline3";
        let diff = generate_diff(old, new, "test.txt");
        assert!(diff.contains("--- test.txt"));
        assert!(diff.contains("+++ test.txt"));
        assert!(diff.contains("-line2"));
        assert!(diff.contains("+line2_modified"));
    }

    #[test]
    fn test_diff_addition() {
        let old = "line1\nline2";
        let new = "line1\nline2\nline3";
        let diff = generate_diff(old, new, "test.txt");
        assert!(diff.contains("+line3"));
    }

    #[test]
    fn test_diff_deletion() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline3";
        let diff = generate_diff(old, new, "test.txt");
        assert!(diff.contains("-line2"));
    }
}
