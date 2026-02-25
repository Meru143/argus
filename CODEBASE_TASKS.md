# Codebase audit: proposed follow-up tasks

## 1) Typo fix task
**Task:** Update the project tagline in `README.md` from "One binary, six tools" to the current number of user-facing subcommands.

**Why:** The README hero copy says "one binary, six tools," but the same document now lists `review`, `describe`, `feedback`, `map`, `search`, `history`, `diff`, `mcp`, and `doctor` (9 total), so the numeric phrase is stale.

## 2) Bug fix task
**Task:** Fix unified diff path parsing so files with spaces/quoted paths are handled correctly in `argus-difflens`.

**Why:** `parse_path` strips `a/` and `b/` prefixes, but does not normalize quoted patch headers such as `--- "a/my file.rs"` and `+++ "b/my file.rs"`. This can propagate incorrect `file_path` values in parsed hunks and downstream review output.

## 3) Code comment / documentation discrepancy task
**Task:** Align wording in `tests/fail_on.rs` with what the tests actually verify, or convert them into true CLI exit-code integration tests.

**Why:** Current test names/comments say "exits zero/one," but the assertions only exercise `Severity::meets_threshold` in-memory (no CLI process execution), which is a behavior/documentation mismatch in the test intent text.

## 4) Test improvement task
**Task:** Add regression tests for quoted/escaped file paths in unified diff fixtures.

**Why:** Existing parser tests cover simple diffs, but there is no fixture that validates `diff --git` + `---/+++` headers containing spaces. A fixture-backed test would prevent regressions once quoted-path parsing is fixed.
