# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/Meru143/argus/compare/argus-difflens-v0.4.1...argus-difflens-v0.5.0) - 2026-02-25

### Fixed

- resolve clippy cmp-owned in deleted file path check
- use normalized new path when detecting deleted files

### Other

- apply rustfmt for difflens parser tests
- add quoted-path diff fixture regression coverage

## [0.3.0](https://github.com/Meru143/argus/compare/argus-difflens-v0.2.2...argus-difflens-v0.3.0) - 2026-02-16

### Fixed

- *(parser)* support patches without 'diff --git' header
