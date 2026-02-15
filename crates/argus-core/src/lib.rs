//! Core types, configuration, and error handling for the Argus platform.
//!
//! This crate provides the shared foundation used by all other Argus crates:
//! - [`ArgusError`] — unified error type using `thiserror`
//! - [`ArgusConfig`] — configuration loaded from `.argus.toml`
//! - Shared types: [`FileNode`], [`DiffHunk`], [`RiskScore`], [`Severity`],
//!   [`ReviewComment`], [`SearchResult`], [`OutputFormat`]

mod config;
mod error;
mod types;

pub use config::{ArgusConfig, EmbeddingConfig, LlmConfig, PathConfig, ReviewConfig, Rule};
pub use error::ArgusError;
pub use types::{
    ChangeType, DiffHunk, FileNode, OutputFormat, ReviewComment, RiskScore, SearchResult, Severity,
};

/// A convenience `Result` type for Argus operations.
pub type Result<T> = std::result::Result<T, ArgusError>;
