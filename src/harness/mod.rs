//! OpenShield AI Harness Core
//!
//! The unified engine that drives OpenShield's agentic behavior.
//!
//! See the `engine`, `event`, and `response` modules for the implementation.

pub mod engine;
pub mod event;
pub mod response;

#[cfg(feature = "web-api")]
pub use engine::{ApprovalGate, ApprovalRequest};
pub use engine::{HarnessConfig, HarnessEngine};
pub use event::HarnessEvent;
