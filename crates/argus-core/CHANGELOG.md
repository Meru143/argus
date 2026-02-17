# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/Meru143/argus/compare/argus-core-v0.3.2...argus-core-v0.4.0) - 2026-02-17

### Fixed

- *(test)* update stale doctest for max_diff_tokens default
- *(config)* increase default max_diff_tokens to 64k to reduce API calls
- *(review)* handle self-review permissions and improve error messages

### Other

- cargo fmt

## [0.3.0](https://github.com/Meru143/argus/compare/argus-core-v0.2.2...argus-core-v0.3.0) - 2026-02-16

### Added

- custom natural language rules via .argus/rules.md
- self-reflection FP filtering, indicatif progress bars, 4 new languages ([#2](https://github.com/Meru143/argus/pull/2))
