//! AI review orchestration combining insights from all Argus modules.
//!
//! The central orchestrator that pipelines DiffLens (triage) → RepoMap
//! (context) → CodeLens (related code) → GitPulse (history) → LLM to
//! produce structured, low-noise code reviews.
