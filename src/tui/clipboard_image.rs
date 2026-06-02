//! Clipboard image paste support for the TUI.
//!
//! Uses arboard to read image data from the system clipboard
//! and convert it to base64 data URLs for multimodal LLM support.

use anyhow::Result;

/// Try to paste an image from the system clipboard.
/// Returns Ok(Some(data_url)) if an image was found,
/// Ok(None) if no image in clipboard,
/// Err if clipboard access failed.
pub fn try_paste_image_from_clipboard() -> Result<Option<String>> {
    use arboard::Clipboard;

    let mut clipboard =
        Clipboard::new().map_err(|e| anyhow::anyhow!("Failed to access clipboard: {}", e))?;

    // Try to get image data from clipboard
    match clipboard.get_image() {
        Ok(image_data) => {
            // Convert arboard's ImageData to bytes
            let bytes = image_data.bytes.into_owned();
            // Determine MIME type from the image data
            let mime_type = detect_image_mime_type(&bytes);
            let data_url = crate::image_utils::encode_image_bytes_to_data_url(&bytes, mime_type);
            Ok(Some(data_url))
        }
        Err(arboard::Error::ContentNotAvailable) => {
            // No image in clipboard — this is fine
            Ok(None)
        }
        Err(e) => Err(anyhow::anyhow!("Clipboard read failed: {}", e)),
    }
}

/// Detect MIME type from image magic bytes.
fn detect_image_mime_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF")
        && bytes.len() > 8
        && &bytes[8..12] == b"WEBP"
    {
        "image/webp"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        "image/png" // Default fallback
    }
}
