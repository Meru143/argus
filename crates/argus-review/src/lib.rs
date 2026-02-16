//! AI review orchestration combining insights from all Argus modules.
//!
//! Provides the review pipeline: LLM client, prompt construction,
//! review orchestration with filtering, and GitHub PR integration.

pub mod github;
pub mod llm;
pub mod patch;
pub mod pipeline;
pub mod prompt;
pub mod sarif;
