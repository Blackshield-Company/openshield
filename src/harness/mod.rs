//! OpenShark AI Harness Core
//!
//! The unified engine that drives OpenShark's agentic behavior.
//!
//! See the `engine`, `event`, and `response` modules for the implementation.

pub mod engine;
pub mod event;
pub mod response;

pub use engine::{HarnessConfig, HarnessEngine};
pub use event::HarnessEvent;

