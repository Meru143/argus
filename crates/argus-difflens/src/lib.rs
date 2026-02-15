//! Diff parsing, complexity scoring, and risk analysis.
//!
//! Provides unified diff parsing, pre-LLM file filtering, complexity
//! scoring, and risk analysis for code changes.

pub mod filter;
pub mod parser;
pub mod risk;
