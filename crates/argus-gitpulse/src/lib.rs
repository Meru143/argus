//! Git history analysis: hotspots, temporal coupling, and knowledge silos.
//!
//! Mines git history using git2 to detect high-churn hotspots, temporally
//! coupled files, and knowledge silos (bus factor) to identify fragile code
//! areas that deserve extra review attention.

pub mod coupling;
pub mod hotspots;
pub mod mining;
pub mod ownership;
