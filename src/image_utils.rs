//! Image encoding utilities for multimodal LLM support.
//!
//! Provides functions to read image files, detect MIME types,
//! and encode them as base64 data URLs suitable for OpenAI-compatible APIs.

use anyhow::{Context, Result};
use base64::engine::{Engine as _, general_purpose::STANDARD};
use std::path::Path;

/// Detect MIME type from file extension.
fn detect_mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        _ => "image/png", // Default fallback
    }
}

/// Read an image file and encode it as a base64 data URL.
///
/// # Arguments
/// * `path` - Path to the image file
///
/// # Returns
/// A string in the format `data:image/png;base64,...` or similar.
///
/// # Errors
/// Returns an error if the file cannot be read.
pub fn encode_image_to_data_url(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read image file: {}", path.display()))?;

    let mime_type = detect_mime_type(path);
    let base64 = STANDARD.encode(&bytes);

    Ok(format!("data:{};base64,{}", mime_type, base64))
}

/// Encode image bytes directly to a data URL.
///
/// # Arguments
/// * `bytes` - Raw image bytes
/// * `mime_type` - MIME type of the image (e.g., "image/png")
///
/// # Returns
/// A string in the format `data:image/png;base64,...`.
pub fn encode_image_bytes_to_data_url(bytes: &[u8], mime_type: &str) -> String {
    let base64 = STANDARD.encode(bytes);
    format!("data:{};base64,{}", mime_type, base64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_mime_type() {
        assert_eq!(detect_mime_type(Path::new("test.png")), "image/png");
        assert_eq!(detect_mime_type(Path::new("test.jpg")), "image/jpeg");
        assert_eq!(detect_mime_type(Path::new("test.jpeg")), "image/jpeg");
        assert_eq!(detect_mime_type(Path::new("test.gif")), "image/gif");
        assert_eq!(detect_mime_type(Path::new("test.webp")), "image/webp");
        assert_eq!(detect_mime_type(Path::new("test.unknown")), "image/png");
    }
}
