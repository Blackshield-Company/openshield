//! Media capabilities — vision, image generation, video, TTS.

use anyhow::Result;
use std::sync::OnceLock;

use crate::tools::Tool;

// ─── Vision / Image Analysis ────────────────────────────────────────────────

pub struct VisionTool;

impl Tool for VisionTool {
    fn name(&self) -> &str {
        "vision"
    }
    fn description(&self) -> &str {
        "Image analysis. Args: <image_path_or_url> [--question <question>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split("--question").collect();
        let image_path = parts.first().unwrap_or(&"").trim();
        let question = parts.get(1).map(|s| s.trim()).unwrap_or("Describe this image.");

        if image_path.is_empty() {
            return Ok("Usage: vision <image_path_or_url> [--question <question>]".to_string());
        }

        // Check if file exists locally
        let path = std::path::Path::new(image_path);
        if path.exists() {
            let metadata = std::fs::metadata(path)?;
            Ok(format!(
                "Vision analysis requested for: {}\nSize: {} bytes\nQuestion: {}\n\nNote: Full vision analysis requires a vision-capable model. Pass the image to the model with your question.",
                image_path,
                metadata.len(),
                question
            ))
        } else {
            Ok(format!(
                "Vision analysis requested for URL: {}\nQuestion: {}\n\nNote: Download and analyze the image using a vision-capable model.",
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
        "Generate images from text prompts. Args: <prompt> [--aspect-ratio landscape|square|portrait]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split("--aspect-ratio").collect();
        let prompt = parts.first().unwrap_or(&"").trim();
        let aspect = parts.get(1).map(|s| s.trim()).unwrap_or("landscape");

        if prompt.is_empty() {
            return Ok("Usage: image_gen <prompt> [--aspect-ratio landscape|square|portrait]".to_string());
        }

        Ok(format!(
            "Image generation requested:\nPrompt: {}\nAspect ratio: {}\n\nNote: Image generation requires a configured provider (FAL, OpenAI, etc.). Set up via environment or config.",
            prompt, aspect
        ))
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
        let question = parts.get(1).map(|s| s.trim()).unwrap_or("Describe this video.");

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
        let output_path = parts.get(1).map(|s| {
            s.split("--voice").next().unwrap_or(s).trim()
        }).unwrap_or("~/.hermes/audio_cache/openshark_tts.mp3");

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
