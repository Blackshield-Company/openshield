//! Inline image display for TUI
//!
//! Since ratatui is text-based, we can't render actual pixels.
//! Instead we show rich metadata indicators and ASCII placeholders.

/// Extract metadata from a base64 data URL.
pub fn extract_image_info(data_url: &str) -> ImageInfo {
    let mut info = ImageInfo::default();

    // Parse MIME type from data URL prefix (data:image/png;base64,...)
    if data_url.starts_with("data:") {
        if let Some(semi) = data_url.strip_prefix("data:").unwrap_or(data_url).find(';') {
            info.mime_type = data_url.strip_prefix("data:").unwrap_or(data_url)[..semi].to_string();
        }
    }

    // Extract base64 payload after the comma
    let payload = data_url.find(',').map(|i| &data_url[i + 1..]).unwrap_or(data_url);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_size() {
        assert_eq!(human_size(0), "0.0 B");
        assert_eq!(human_size(512), "512.0 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
        assert_eq!(human_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_image_info_format_indicator() {
        let info = ImageInfo {
            mime_type: "image/png".to_string(),
            size_bytes: 1234,
            size_human: "1.2 KB".to_string(),
            width: 1920,
            height: 1080,
        };
        let indicator = info.format_indicator();
        assert!(indicator.contains("PNG"));
        assert!(indicator.contains("1920x1080"));
        assert!(indicator.contains("1.2 KB"));
    }

    #[test]
    fn test_ascii_placeholder() {
        let info = ImageInfo {
            mime_type: "image/png".to_string(),
            size_bytes: 0,
            size_human: "0 B".to_string(),
            width: 100,
            height: 100,
        };
        let placeholder = info.ascii_placeholder();
        assert!(placeholder.len() >= 4); // top border, lines, bottom border, dimensions
        assert!(placeholder[0].starts_with('┌'));
        assert!(placeholder[placeholder.len() - 2].starts_with('└'));
        assert!(placeholder[placeholder.len() - 1].contains("100x100"));
    }

    #[test]
    fn test_png_dimension_detection() {
        // Minimal valid PNG header with 2x3 dimensions
        let mut png = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
            0x00, 0x00, 0x00, 0x0D, // IHDR length
            0x49, 0x48, 0x44, 0x52, // IHDR
        ];
        png.extend_from_slice(&2u32.to_be_bytes()); // width = 2
        png.extend_from_slice(&3u32.to_be_bytes()); // height = 3
        let dims = detect_png_dimensions(&png);
        assert_eq!(dims, Some((2, 3)));
    }

    #[test]
    fn test_gif_dimension_detection() {
        let gif = b"GIF89a\x02\x00\x03\x00"; // 2x3 GIF
        let dims = detect_gif_dimensions(gif);
        assert_eq!(dims, Some((2, 3)));
    }

    #[test]
    fn test_bmp_dimension_detection() {
        let mut bmp = vec![b'B', b'M'];
        bmp.extend_from_slice(&54u32.to_le_bytes()); // file size
        bmp.extend_from_slice(&0u32.to_le_bytes()); // reserved
        bmp.extend_from_slice(&54u32.to_le_bytes()); // data offset
        bmp.extend_from_slice(&40u32.to_le_bytes()); // header size
        bmp.extend_from_slice(&5u32.to_le_bytes()); // width = 5
        bmp.extend_from_slice(&7u32.to_le_bytes()); // height = 7
        let dims = detect_bmp_dimensions(&bmp);
        assert_eq!(dims, Some((5, 7)));
    }
}
