//! OpenShark Capability Suite — All tools implemented natively in Rust.
//!
//! No external CLI dependencies. Every tool is a native struct implementing
//! the `Tool` trait. Expensive resources (HTTP clients, DB connections) are
//! lazily initialized on first use via `OnceLock` or `Mutex<Option<T>>`.
//!
//! ## Tool Categories
//! - **web**: Web search, browser automation, X/Twitter search
//! - **media**: Vision, image generation, video analysis/generation, TTS
//! - **memory**: Persistent memory, session search, context engine
//! - **productivity**: Todo lists, cron jobs, skill management
//! - **communication**: Cross-platform messaging
//! - **smart_home**: Home Assistant, Spotify
//! - **platform**: Yuanbao, computer use
//! - **agentic**: Mixture of Agents, delegation, clarifying questions
//! - **execution**: Python code execution

pub mod agentic;
pub mod communication;
pub mod execution;
pub mod media;
pub mod memory;
pub mod platform;
pub mod productivity;
pub mod smart_home;
pub mod web;

use crate::tools::Tool;
use std::sync::{Arc, OnceLock};

/// Global singleton registry for all capability tools.
static CAPABILITY_REGISTRY: OnceLock<CapabilityRegistry> = OnceLock::new();

/// Registry holding all capability tools.
pub struct CapabilityRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl CapabilityRegistry {
    /// Build the registry with all capability tools.
    pub fn new() -> Self {
        let mut tools: Vec<Arc<dyn Tool>> = Vec::with_capacity(24);

        // Web & Search
        tools.push(Arc::new(web::WebSearchTool));
        tools.push(Arc::new(web::BrowserTool));
        tools.push(Arc::new(web::XSearchTool));

        // Media
        tools.push(Arc::new(media::VisionTool));
        tools.push(Arc::new(media::ImageGenTool));
        tools.push(Arc::new(media::VideoTool));
        tools.push(Arc::new(media::VideoGenTool));
        tools.push(Arc::new(media::TtsTool));

        // Memory & Context
        tools.push(Arc::new(memory::MemoryTool));
        tools.push(Arc::new(memory::SessionSearchTool));
        tools.push(Arc::new(memory::ContextEngineTool));

        // Productivity
        tools.push(Arc::new(productivity::TodoTool));
        tools.push(Arc::new(productivity::CronjobTool));
        tools.push(Arc::new(productivity::SkillsTool));

        // Communication
        tools.push(Arc::new(communication::MessagingTool));

        // Smart Home
        tools.push(Arc::new(smart_home::HomeAssistantTool));
        tools.push(Arc::new(smart_home::SpotifyTool));

        // Platform
        tools.push(Arc::new(platform::YuanbaoTool));
        tools.push(Arc::new(platform::ComputerUseTool));

        // Agentic
        tools.push(Arc::new(agentic::MoaTool));
        tools.push(Arc::new(agentic::DelegationTool));
        tools.push(Arc::new(agentic::ClarifyTool));

        // Execution
        tools.push(Arc::new(execution::CodeExecutionTool));

        Self { tools }
    }

    /// Get all capability tools.
    pub fn tools(&self) -> &[Arc<dyn Tool>] {
        &self.tools
    }

    /// Find a tool by name.
    pub fn find(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name).cloned()
    }

    /// Get tool names and descriptions for system prompts.
    pub fn tool_descriptions(&self) -> Vec<(String, String)> {
        self.tools
            .iter()
            .map(|t| (t.name().to_string(), t.description().to_string()))
            .collect()
    }
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the global capability registry.
pub fn global_registry() -> &'static CapabilityRegistry {
    CAPABILITY_REGISTRY.get_or_init(CapabilityRegistry::new)
}

/// Get all capability tools as a vector.
pub fn get_capability_tools() -> Vec<Arc<dyn Tool>> {
    global_registry().tools().to_vec()
}

/// Find a capability tool by name.
pub fn find_capability_tool(name: &str) -> Option<Arc<dyn Tool>> {
    global_registry().find(name)
}
