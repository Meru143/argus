//! Diff parsing, complexity scoring, and risk analysis.
//!
//! Provides unified diff parsing and basic risk scoring for code changes.
//! Uses size, diffusion, and file-type heuristics to compute risk scores.

pub mod parser;
pub mod risk;
