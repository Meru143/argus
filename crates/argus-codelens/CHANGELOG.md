# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/Meru143/argus/compare/argus-codelens-v0.3.2...argus-codelens-v0.4.0) - 2026-02-17

### Added

- implement learning from feedback (argus feedback)

### Fixed

- *(review)* improve permission error handling and add store tests
- *(store)* cast limit to i64 in get_negative_feedback
- *(store)* propagate database errors in stats queries instead of unwrapping
- *(store)* ensure feedback uniqueness by comment_id
- *(store)* add UNIQUE constraint to feedback and use INSERT OR REPLACE

### Other

- fix IndexStats doctest initialization
- cargo fmt

## [0.3.0](https://github.com/Meru143/argus/compare/argus-codelens-v0.2.2...argus-codelens-v0.3.0) - 2026-02-16

### Added

- add PHP, Kotlin, Swift tree-sitter support (9→12 languages)
- self-reflection FP filtering, indicatif progress bars, 4 new languages ([#2](https://github.com/Meru143/argus/pull/2))
