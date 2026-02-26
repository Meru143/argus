# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.2](https://github.com/Meru143/argus/compare/argus-review-v0.5.1...argus-review-v0.5.2) - 2026-02-26

### Other

- Fix multiple bugs and performance issues across the workspace ([#60](https://github.com/Meru143/argus/pull/60))

## [0.5.0](https://github.com/Meru143/argus/compare/argus-review-v0.4.1...argus-review-v0.5.0) - 2026-02-25

### Other

- Retry LLM calls on 429s and track retry telemetry ([#57](https://github.com/Meru143/argus/pull/57))
- Sanitize and cap LLM provider error bodies ([#55](https://github.com/Meru143/argus/pull/55))

## [0.4.0](https://github.com/Meru143/argus/compare/argus-review-v0.3.2...argus-review-v0.4.0) - 2026-02-17

### Added

- implement learning from feedback (argus feedback)

### Fixed

- *(review)* improve permission error handling and add store tests
- *(llm)* increase Gemini retry backoff to 10s/5-retries
- *(lint)* collapse nested if in retry logic
- *(llm)* add exponential backoff retry for Gemini 429 errors
- *(review)* increase rate limit delay for free-tier Gemini
- *(review)* remove useless format! to satisfy clippy
- *(review)* use local build for review and add rate limit backing off
- *(review)* handle self-review permissions and improve error messages
- *(review)* use COMMENT event instead of REQUEST_CHANGES to avoid CI permission errors

### Other

- cargo fmt fix for llm.rs
- cargo fmt
- *(review)* restore REQUEST_CHANGES logic (AI review flagged safety regression)

## [0.3.1](https://github.com/Meru143/argus/compare/argus-review-v0.3.0...argus-review-v0.3.1) - 2026-02-16

### Added

- add local LLM support via Ollama

### Other

- fix cargo fmt

## [0.3.0](https://github.com/Meru143/argus/compare/argus-review-v0.2.2...argus-review-v0.3.0) - 2026-02-16

### Added

- implement learning from feedback (argus feedback)
- implement hotspot-aware review prioritization
- implement incremental review (--incremental)
- add PR description generation (argus describe)
- self-reflection FP filtering, indicatif progress bars, 4 new languages ([#2](https://github.com/Meru143/argus/pull/2))
