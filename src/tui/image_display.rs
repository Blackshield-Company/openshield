//! Inline image display for TUI
//!
//! Since ratatui is text-based, we can't render actual pixels.
//! Instead we show rich metadata indicators and ASCII placeholders.

/// Extract metadata from a base64 data URL.
pub fn extract_image_info(data_url: &str) -> ImageInfo {
    let mut info = ImageInfo::default();

    // Parse MIME type from data URL prefix
    if let Some(semi) = data_url.find(';') {
        info.mime_type = data_url[5..semi].to_string(); // skip "data:"
    }

    // Extract base64 payload
    let payload = if let Some(comma) = data_url.find(',') {
        &data_url[comma + 1..]
    } else {
        data_url
    };

    // Decode base64 to get raw bytes for analysis
    if let Ok(bytes) = base64_decode(payload) {
        info.size_bytes = bytes.len();
        info.size_human = human_size(bytes.len());

        // Try to detect dimensions from image headers
        if let Some((w, h)) = detect_dimensions(&bytes, &info.mime_type) {
            info.width = w;
            info.height = h;
        }
    }

    info
}

#[derive(Debug, Clone, Default)]
pub struct ImageInfo {
    pub mime_type: String,
    pub size_bytes: usize,
    pub size_human: String,
    pub width: u32,
    pub height: u32,
}

impl ImageInfo {
    /// Format as a compact indicator line for TUI display.
    pub fn format_indicator(&self) -> String {
        let dim = if self.width > 0 && self.height > 0 {
            format!("{}x{}", self.width, self.height)
        } else {
            "?x?".to_string()
        };
        format!(
            "📎 {} | {} | {}",
            self.mime_type.replace("image/", "").to_uppercase(),
            dim,
            self.size_human
        )
    }

    /// Generate an ASCII art placeholder representing the image.
    pub fn ascii_placeholder(&self) -> Vec<String> {
        let w = self.width;
        let h = self.height;
        let aspect = if h > 0 {
            w as f32 / h as f32
        } else {
            1.0
        };

        // Target display size in characters
        let target_width = 40u32.min(w.max(10));
        let target_height = ((target_width as f32 / aspect) * 0.5) as u32; // *0.5 for char aspect ratio
        let target_height = target_height.clamp(3, 12);

        let mut lines = Vec::new();
        lines.push("┌".to_string() + &"─".repeat(target_width as usize) + "┐");
        for _ in 0..target_height {
            lines.push("│".to_string() + &"░".repeat(target_width as usize) + "│");
        }
        lines.push("└".to_string() + &"─".repeat(target_width as usize) + "┘");
        lines.push(format!("  {}x{} px", w, h));

        lines
    }
}

fn human_size(bytes: usize) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    format!("{:.1} {}", size, UNITS[unit_idx])
}

fn base64_decode(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.decode(input.replace(['\n', '\r', ' '], ""))
}

/// Detect image dimensions from raw bytes.
fn detect_dimensions(bytes: &[u8], mime: &str) -> Option<(u32, u32)> {
    match mime {
        "image/png" => detect_png_dimensions(bytes),
        "image/jpeg" => detect_jpeg_dimensions(bytes),
        "image/gif" => detect_gif_dimensions(bytes),
        "image/webp" => detect_webp_dimensions(bytes),
        "image/bmp" => detect_bmp_dimensions(bytes),
        _ => detect_png_dimensions(bytes)
            .or_else(|| detect_jpeg_dimensions(bytes))
            .or_else(|| detect_gif_dimensions(bytes)),
    }
}

fn detect_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // PNG: width at bytes 16-19, height at 20-23 (big-endian)
    if bytes.len() >= 24 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        Some((w, h))
    } else {
        None
    }
}

fn detect_jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // JPEG: scan for SOF0/SOF2 markers (0xFF 0xC0 / 0xFF 0xC2)
    let mut i = 2;
    while i + 9 < bytes.len() {
        if bytes[i] == 0xFF {
            match bytes[i + 1] {
                0xC0 | 0xC2 => {
                    let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                    let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                    return Some((w, h));
                }
                0xD9 => break, // EOI
                0xD8 => i += 2, // SOI
                _ => {
                    if i + 3 < bytes.len() {
                        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
                        i += 2 + len;
                    } else {
                        break;
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

fn detect_gif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // GIF: width at bytes 6-7 (little-endian), height at 8-9
    if bytes.len() >= 10 && (&bytes[0..6] == b"GIF87a" || &bytes[0..6] == b"GIF89a") {
        let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        Some((w, h))
    } else {
        None
    }
}

fn detect_webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // VP8 chunk: "VP8 " at offset 12, dimensions in VP8 bitstream
    if bytes.len() >= 30 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        if &bytes[12..16] == b"VP8 " {
            // Simple VP8: dimensions at bytes 26-29 (little-endian, 14-bit each)
            let b = &bytes[26..30];
            let bits = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            let w = bits & 0x3FFF;
            let h = (bits >> 16) & 0x3FFF;
            Some((w, h))
        } else if &bytes[12..16] == b"VP8L" {
            // VP8 Lossless: dimensions at bytes 21-24
            let b = &bytes[21..25];
            let bits = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            Some((w, h))
        } else {
            None
        }
    } else {
        None
    }
}

fn detect_bmp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // BMP: width at bytes 18-21, height at 22-25 (little-endian)
    if bytes.len() >= 26 && &bytes[0..2] == b"BM" {
        let w = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
        let h = u32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
        Some((w, h))
    } else {
        None
    }
}
