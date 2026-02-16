# Argus Improvement Sprint — Plan

## Feature 1: Self-Reflection / FP Filtering (HIGH PRIORITY)

**Problem**: #1 user complaint is noise. AI reviews generate too many false positives — style nits, low-confidence hunches, and comments about code the LLM can't fully verify.

**Solution**: After the initial LLM review generates comments, run a second LLM pass that evaluates each comment against the original diff. The second pass:
1. Receives the original diff + all generated comments
2. Scores each comment 1-10 for relevance and correctness
3. Filters out comments scoring below threshold (default: 7)
4. Can also re-classify severity (e.g., downgrade a "bug" to "suggestion")

**Implementation**:
- New function `self_reflect()` in `pipeline.rs`
- New prompt `build_self_reflection_prompt()` in `prompt.rs`
- New config field `self_reflection: bool` in `ReviewConfig` (default: true)
- New config field `self_reflection_score_threshold: u8` (default: 7)
- Stats tracked: `comments_reflected_out: usize`
- Integrates between step 3 (deduplicate) and step 4 (filter_and_sort)
- Uses same LlmClient — one additional API call

**Research**: PR-Agent uses `new_score_mechanism` with thresholds. BugBot achieves 70%+ fix rate through aggressive filtering. Our approach: LLM-as-judge on its own output with structured scoring.

## Feature 2: Progress Bars with indicatif (MEDIUM PRIORITY)

**Problem**: Current progress is raw `eprintln!` text. Looks unprofessional and doesn't show elapsed time or spinners.

**Solution**: Replace `eprintln!` progress with `indicatif` progress bars/spinners.
- Spinner for single-group reviews
- Multi-progress bar for multi-group reviews showing each group
- Elapsed time display
- Only when stderr is a terminal (already checked)

**Implementation**:
- Add `indicatif` dependency to `argus-review`
- Refactor progress output in `pipeline.rs` review method
- Use `ProgressBar` with spinner style for LLM calls
- Use `MultiProgress` for multi-group reviews

## Feature 3: More Tree-sitter Languages (LOW PRIORITY)

**Problem**: Only supports Rust/Python/TypeScript/JavaScript/Go. Missing Java, C/C++, Ruby, PHP, Kotlin, Swift.

**Solution**: Add tree-sitter grammars for Java, C, C++, Ruby. (PHP/Kotlin/Swift have less mature grammars.)

**Implementation**:
- Add `tree-sitter-java`, `tree-sitter-c`, `tree-sitter-cpp`, `tree-sitter-ruby` to workspace deps
- Add language detection and parser setup in `parser.rs` and `walker.rs`
- Add node type mappings for definitions/references per language
- Tests for each new language

## Execution Order
1. Self-reflection (biggest impact)
2. Progress bars (quick UX win)
3. More languages (incremental)

Each feature = one commit.
