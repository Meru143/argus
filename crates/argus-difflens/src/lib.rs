//! Diff parsing, complexity scoring, and risk analysis.
//!
//! Analyzes git diffs to compute risk scores based on size, complexity delta,
//! diffusion, coverage, and file-type risk. Uses git2 for diff operations and
//! tree-sitter for cyclomatic complexity measurement.
