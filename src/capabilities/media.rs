//! Media capabilities — vision, image generation, video, TTS.

use anyhow::{Context, Result};

use crate::tools::Tool;

// ─── Vision / Image Analysis ────────────────────────────────────────────────

pub struct VisionTool;

impl Tool for VisionTool {
    fn name(&self) -> &str {
        "vision"
    }
    fn description(&self) -> &str {
        "Image analysis. Args: <image_path> [--question <question>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split("--question").collect();
        let image_path = parts.first().unwrap_or(&"").trim();
        let question = parts
            .get(1)
            .map(|s| s.trim())
            .unwrap_or("Describe this image.");

        if image_path.is_empty() {
            return Ok("Usage: vision <image_path> [--question <question>]\nSupports: PNG, JPG, GIF, WebP. Use after browser --snapshot for page analysis.".to_string());
        }

        // Read and encode the image as base64 data URL for vision models
        let path = std::path::Path::new(image_path);
        if path.exists() {
            let data = std::fs::read(path)
                .with_context(|| format!("Failed to read image: {}", image_path))?;

            // Detect MIME type from extension
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png")
                .to_lowercase();
            let mime = match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                _ => "image/png",
            };

            use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
            let b64 = BASE64.encode(&data);
            let data_url = format!("data:{};base64,{}", mime, b64);

            Ok(format!(
                "Image loaded: {} ({} bytes, {})Question: {}\n--- VISION DATA (base64 data URL) ---\n{}\nUse this data URL as an image attachment in your next message to analyze it with a vision-capable model.",
                image_path,
                data.len(),
                mime,
                question,
                data_url
            ))
        } else {
            Ok(format!(
                "Image not found: {}\nQuestion: {}\n\nTip: Use browser --snapshot <url> to capture a page screenshot first, then vision /tmp/openshark_snapshot.png.",
                image_path, question
            ))
        }
    }
}

// ─── Image Generation ───────────────────────────────────────────────────────

pub struct ImageGenTool;

impl Tool for ImageGenTool {
    fn name(&self) -> &str {
        "image_gen"
    }
    fn description(&self) -> &str {
        "Generate images from text prompts via FAL. Args: <prompt> [--aspect-ratio landscape|square|portrait]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split("--aspect-ratio").collect();
        let prompt = parts.first().unwrap_or(&"").trim();
        let aspect = parts.get(1).map(|s| s.trim()).unwrap_or("landscape");

        if prompt.is_empty() {
            return Ok(
                "Usage: image_gen <prompt> [--aspect-ratio landscape|square|portrait]".to_string(),
            );
        }

        // Map aspect ratio to FAL dimensions
        let (width, height) = match aspect {
            "square" => (1024, 1024),
            "portrait" => (768, 1344),
            _ => (1344, 768), // landscape default
        };

        // Load FAL key from env or fal.env
        let fal_key = std::env::var("FAL_KEY")
            .or_else(|_| std::env::var("FAL_API_KEY"))
            .or_else(|_| {
                // Try loading from fal.env in config dir
                let env_path = dirs::config_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("openshark/fal.env");
                if let Ok(content) = std::fs::read_to_string(&env_path) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.starts_with("FAL_KEY=") {
                            return Ok(line.trim_start_matches("FAL_KEY=").to_string());
                        }
                        if line.starts_with("FAL_API_KEY=") {
                            return Ok(line.trim_start_matches("FAL_API_KEY=").to_string());
                        }
                    }
                }
                Err(std::env::VarError::NotPresent)
            });

        let fal_key = match fal_key {
            Ok(k) => k,
            Err(_) => {
                return Ok(
                    "No FAL_KEY found. Set FAL_KEY environment variable or create ~/.config/openshark/fal.env".to_string()
                );
            }
        };

        // Call FAL API
        let client = reqwest::blocking::Client::new();
        let body = serde_json::json!({
            "prompt": prompt,
            "image_size": {
                "width": width,
                "height": height
            },
            "num_images": 1,
            "enable_safety_checker": false
        });

        let response = client
            .post("https://api.fal.ai/v1/images/generations")
            .header("Authorization", format!("Key {}", fal_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send();

        match response {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().unwrap_or_default();
                if status.is_success() {
                    // Parse the response to extract image URL
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(images) = json.get("images").and_then(|i| i.as_array()) {
                            if let Some(first) = images.first() {
                                if let Some(url) = first.get("url").and_then(|u| u.as_str()) {
                                    return Ok(format!(
                                        "Image generated!\nPrompt: {}\nAspect: {} ({}x{})\nURL: {}\n\nMEDIA:{}",
                                        prompt, aspect, width, height, url, url
                                    ));
                                }
                            }
                        }
                        // Fallback: return raw JSON if we can't parse
                        Ok(format!(
                            "Image generated!\nPrompt: {}\nAspect: {} ({}x{})\nResponse: {}",
                            prompt, aspect, width, height, text
                        ))
                    } else {
                        Ok(format!(
                            "Image generated!\nPrompt: {}\nAspect: {} ({}x{})\nResponse: {}",
                            prompt, aspect, width, height, text
                        ))
                    }
                } else {
                    Ok(format!(
                        "FAL API error ({}): {}\n\nBody: {}",
                        status, status.canonical_reason().unwrap_or("Unknown"), text
                    ))
                }
            }
            Err(e) => {
                Ok(format!("Failed to call FAL API: {}", e))
            }
        }
    }
}

// ─── Video Analysis ─────────────────────────────────────────────────────────

pub struct VideoTool;

impl Tool for VideoTool {
    fn name(&self) -> &str {
        "video"
    }
    fn description(&self) -> &str {
        "Video analysis. Args: <video_path_or_url> [--question <question>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split("--question").collect();
        let video_path = parts.first().unwrap_or(&"").trim();
        let question = parts
            .get(1)
            .map(|s| s.trim())
            .unwrap_or("Describe this video.");

        if video_path.is_empty() {
            return Ok("Usage: video <video_path_or_url> [--question <question>]".to_string());
        }

        Ok(format!(
            "Video analysis requested for: {}\nQuestion: {}\n\nNote: Video analysis requires a video-capable model or frame extraction pipeline.",
            video_path, question
        ))
    }
}

// ─── Video Generation ───────────────────────────────────────────────────────

pub struct VideoGenTool;

impl Tool for VideoGenTool {
    fn name(&self) -> &str {
        "video_gen"
    }
    fn description(&self) -> &str {
        "Generate video from text prompts. Args: <prompt> [--duration <secs>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split("--duration").collect();
        let prompt = parts.first().unwrap_or(&"").trim();
        let _duration = parts.get(1).map(|s| s.trim()).unwrap_or("5");

        if prompt.is_empty() {
            return Ok("Usage: video_gen <prompt> [--duration <secs>]".to_string());
        }

        Ok(format!(
            "Video generation requested:\nPrompt: {}\n\nNote: Video generation requires a configured provider (FAL, Runway, etc.).",
            prompt
        ))
    }
}

// ─── Text-to-Speech ─────────────────────────────────────────────────────────

pub struct TtsTool;

impl Tool for TtsTool {
    fn name(&self) -> &str {
        "tts"
    }
    fn description(&self) -> &str {
        "Text-to-speech conversion. Args: <text> [--output <path>] [--voice <voice_id>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split("--output").collect();
        let text = parts.first().unwrap_or(&"").trim();
        let output_path = parts
            .get(1)
            .map(|s| s.split("--voice").next().unwrap_or(s).trim())
            .unwrap_or("~/.hermes/audio_cache/openshark_tts.mp3");

        if text.is_empty() {
            return Ok("Usage: tts <text> [--output <path>] [--voice <voice_id>]".to_string());
        }

        let expanded = shellexpand::tilde(output_path);
        Ok(format!(
            "TTS requested:\nText: {}\nOutput: {}\n\nNote: TTS requires a configured provider (OpenAI, ElevenLabs, Edge, etc.).",
            text.chars().take(100).collect::<String>(),
            expanded
        ))
    }
}
