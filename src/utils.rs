//! Shared utility functions.

/// Truncate a string to at most `max_bytes` bytes, respecting UTF-8 char boundaries.
///
/// Unlike `&s[..max_bytes]`, this will never panic by splitting a multi-byte character.
/// Returns a String of at most `max_bytes` bytes.
pub fn truncate_str(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    // Find the last char boundary at or before max_bytes
    match s
        .char_indices()
        .take_while(|(idx, _)| *idx <= max_bytes)
        .last()
    {
        Some((idx, c)) => {
            let end = idx + c.len_utf8();
            if end <= max_bytes {
                s[..end].to_string()
            } else {
                s[..idx].to_string()
            }
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_ascii() {
        assert_eq!(truncate_str("hello world", 5), "hello");
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_utf8_multibyte() {
        // "café" = c(1) a(1) f(1) é(2) = 5 bytes
        let s = "café";
        assert_eq!(truncate_str(s, 5), "café");
        // Truncate at byte 4 — splits é, should land at byte 3 (just "caf")
        assert_eq!(truncate_str(s, 4), "caf");
        assert_eq!(truncate_str(s, 3), "caf");
        assert_eq!(truncate_str(s, 2), "ca");
    }

    #[test]
    fn test_truncate_emoji() {
        // "🦞🎹" = 4 bytes each = 8 bytes total
        let s = "🦞🎹";
        assert_eq!(truncate_str(s, 8), "🦞🎹");
        assert_eq!(truncate_str(s, 5), "🦞");
        assert_eq!(truncate_str(s, 4), "🦞");
        assert_eq!(truncate_str(s, 3), "");
    }

    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate_str("", 0), "");
        assert_eq!(truncate_str("", 10), "");
    }

    #[test]
    fn test_truncate_zero() {
        assert_eq!(truncate_str("hello", 0), "");
    }
}
