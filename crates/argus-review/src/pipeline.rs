use std::collections::HashMap;
use std::fmt;
use std::io::IsTerminal;
use std::path::Path;

use argus_core::{ArgusError, OutputFormat, ReviewComment, ReviewConfig, Rule, Severity};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Serialize;

use argus_difflens::filter::{DiffFilter, SkippedFile};
use argus_difflens::parser::FileDiff;

use crate::llm::{ChatMessage, LlmClient, Role};
use crate::prompt;

/// Result of a completed code review.
///
/// # Examples
///
/// ```
/// use argus_review::pipeline::{ReviewResult, ReviewStats};
///
/// let result = ReviewResult {
///     comments: vec![],
///     filtered_comments: vec![],
///     summary: None,
///     stats: ReviewStats {
///         files_reviewed: 0,
///         files_skipped: 0,
///         total_hunks: 0,
///         comments_generated: 0,
///         comments_filtered: 0,
///         comments_deduplicated: 0,
///         comments_reflected_out: 0,
///         skipped_files: vec![],
///         model_used: "gpt-4o".into(),
///         llm_calls: 0,
///         file_groups: vec![],
///     },
/// };
/// assert!(result.comments.is_empty());
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    /// Filtered and sorted review comments.
    pub comments: Vec<ReviewComment>,
    /// Comments that were removed by filtering, with reasons.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub filtered_comments: Vec<FilteredComment>,
    /// High-level summary of the review findings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Statistics about the review run.
    pub stats: ReviewStats,
}

/// A review comment that was removed by the filtering pipeline.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_core::{ReviewComment, Severity};
/// use argus_review::pipeline::FilteredComment;
///
/// let fc = FilteredComment {
///     comment: ReviewComment {
///         file_path: PathBuf::from("src/lib.rs"),
///         line: 10,
///         severity: Severity::Info,
///         message: "minor note".into(),
///         confidence: 95.0,
///         suggestion: None,
///         patch: None,
///         rule: None,
///     },
///     reason: "below confidence threshold".into(),
/// };
/// assert!(fc.reason.contains("confidence"));
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilteredComment {
    /// The original comment that was filtered out.
    pub comment: ReviewComment,
    /// Why this comment was filtered.
    pub reason: String,
}

/// Statistics about a review run.
///
/// # Examples
///
/// ```
/// use argus_review::pipeline::ReviewStats;
///
/// let stats = ReviewStats {
///     files_reviewed: 3,
///     files_skipped: 1,
///     total_hunks: 5,
///     comments_generated: 10,
///     comments_filtered: 7,
///     comments_deduplicated: 1,
///     comments_reflected_out: 2,
///     skipped_files: vec![],
///     model_used: "gpt-4o".into(),
///     llm_calls: 2,
///     file_groups: vec![],
/// };
/// assert_eq!(stats.files_reviewed, 3);
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewStats {
    /// Number of files that were reviewed.
    pub files_reviewed: usize,
    /// Number of files skipped by the pre-LLM filter.
    pub files_skipped: usize,
    /// Total number of diff hunks sent.
    pub total_hunks: usize,
    /// Raw comments from the LLM before filtering.
    pub comments_generated: usize,
    /// Comments removed by confidence/severity filters.
    pub comments_filtered: usize,
    /// Duplicate comments merged.
    pub comments_deduplicated: usize,
    /// Comments removed by self-reflection pass.
    pub comments_reflected_out: usize,
    /// Files that were skipped with reasons.
    #[serde(skip)]
    pub skipped_files: Vec<SkippedFile>,
    /// Model identifier used for the review.
    pub model_used: String,
    /// Number of LLM API calls made.
    pub llm_calls: usize,
    /// Cross-file groups used during review (for verbose output).
    #[serde(skip)]
    pub file_groups: Vec<Vec<String>>,
}

/// Review orchestrator that drives the full review pipeline.
///
/// Concatenates diffs, sends them to the LLM, parses the response,
/// and applies confidence/severity filtering.
pub struct ReviewPipeline {
    llm: LlmClient,
    config: ReviewConfig,
    rules: Vec<Rule>,
}

impl ReviewPipeline {
    /// Create a new pipeline from an LLM client, review config, and custom rules.
    pub fn new(llm: LlmClient, config: ReviewConfig, rules: Vec<Rule>) -> Self {
        Self { llm, config, rules }
    }

    /// Run a review on parsed diffs and return filtered comments.
    ///
    /// When `repo_path` is provided, a repo map is generated using the diff
    /// file paths as focus files and included in the LLM prompt for context.
    ///
    /// The pipeline:
    /// 1. Pre-filters diffs (lock files, generated, vendored, etc.)
    /// 2. Splits large diffs into per-file LLM calls if needed
    /// 3. Deduplicates comments
    /// 4. Applies confidence/severity filtering
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Llm`] if the LLM call fails.
    pub async fn review(
        &self,
        diffs: &[FileDiff],
        repo_path: Option<&Path>,
    ) -> Result<ReviewResult, ArgusError> {
        // 1. Pre-filter diffs
        let diff_filter = DiffFilter::from_config(&self.config);
        let filter_result = diff_filter.filter(diffs.to_vec());
        let kept_diffs = filter_result.kept;
        let skipped_files = filter_result.skipped;
        let files_skipped = skipped_files.len();

        let files_reviewed = kept_diffs.len();
        let total_hunks: usize = kept_diffs.iter().map(|d| d.hunks.len()).sum();

        if kept_diffs.is_empty() {
            return Ok(ReviewResult {
                comments: Vec::new(),
                filtered_comments: Vec::new(),
                summary: None,
                stats: ReviewStats {
                    files_reviewed: 0,
                    files_skipped,
                    total_hunks: 0,
                    comments_generated: 0,
                    comments_filtered: 0,
                    comments_deduplicated: 0,
                    comments_reflected_out: 0,
                    skipped_files,
                    model_used: self.llm.model().to_string(),
                    llm_calls: 0,
                    file_groups: vec![],
                },
            });
        }

        // Generate repo map if a repo path is provided
        let repo_map = if let Some(root) = repo_path {
            let focus_files: Vec<std::path::PathBuf> =
                kept_diffs.iter().map(|d| d.new_path.clone()).collect();
            match argus_repomap::generate_map(root, 1024, &focus_files, OutputFormat::Text) {
                Ok(map) if !map.is_empty() => Some(map),
                _ => None,
            }
        } else {
            None
        };

        // Search for related code context if an index exists
        let related_code = if let Some(root) = repo_path {
            let index_path = root.join(".argus/index.db");
            if index_path.exists() {
                build_related_code_context(&kept_diffs, &index_path)
            } else {
                None
            }
        } else {
            None
        };

        // Build git history context if repo is available
        let history_context = if let Some(root) = repo_path {
            build_history_context(&kept_diffs, root)
        } else {
            None
        };

        // 2. Decide whether to split or send as one call
        let diff_text = diffs_to_text(&kept_diffs);
        let total_tokens = estimate_tokens(&diff_text);

        let system = prompt::build_system_prompt(&self.config, &self.rules);
        let mut all_comments = Vec::new();
        let mut llm_calls: usize = 0;
        let mut file_groups: Vec<Vec<String>> = Vec::new();

        if total_tokens > self.config.max_diff_tokens && kept_diffs.len() > 1 {
            // Split into groups and review each group
            let groups = if self.config.cross_file {
                group_related_diffs(&kept_diffs, self.config.max_diff_tokens)
            } else {
                // Disable grouping: each file is its own group
                kept_diffs.iter().map(|d| vec![d]).collect()
            };

            // Record groups for verbose output
            for group in &groups {
                let names: Vec<String> = group
                    .iter()
                    .map(|d| d.new_path.to_string_lossy().into_owned())
                    .collect();
                file_groups.push(names);
            }

            let is_tty = std::io::stderr().is_terminal();
            let group_count = groups.len();
            let mp = MultiProgress::new();

            let main_pb = if is_tty {
                let file_count: usize = groups.iter().map(|g| g.len()).sum();
                let pb = mp.add(ProgressBar::new(group_count as u64));
                pb.set_style(
                    ProgressStyle::with_template(
                        "{spinner:.cyan} Reviewing {msg} [{bar:20.cyan/dim}] {pos}/{len} groups ({elapsed})"
                    )
                    .unwrap()
                    .progress_chars("━╸─"),
                );
                pb.set_message(format!("{file_count} files"));
                pb.enable_steady_tick(std::time::Duration::from_millis(120));
                Some(pb)
            } else {
                None
            };

            for (i, group) in groups.iter().enumerate() {
                let group_pb = if is_tty {
                    let label = group_display_name(group.as_slice());
                    let pb = mp.add(ProgressBar::new_spinner());
                    pb.set_style(ProgressStyle::with_template("  {spinner:.dim} {msg}").unwrap());
                    pb.set_message(format!("[{}/{}] {label}...", i + 1, group_count));
                    pb.enable_steady_tick(std::time::Duration::from_millis(120));
                    Some(pb)
                } else {
                    None
                };

                let group_diff_text = diffs_to_text(group);
                let is_cross_file = group.len() > 1;
                let user = prompt::build_review_prompt(
                    &group_diff_text,
                    repo_map.as_deref(),
                    related_code.as_deref(),
                    history_context.as_deref(),
                    None,
                    is_cross_file,
                );

                let messages = vec![
                    ChatMessage {
                        role: Role::System,
                        content: system.clone(),
                    },
                    ChatMessage {
                        role: Role::User,
                        content: user,
                    },
                ];

                let response = self.llm.chat(messages).await?;
                llm_calls += 1;
                let mut parsed = prompt::parse_review_response(&response)?;

                if let Some(pb) = &group_pb {
                    let label = group_display_name(group.as_slice());
                    let comment_count = parsed.len();
                    pb.finish_with_message(format!(
                        "[{}/{}] {label} → {comment_count} comment{}",
                        i + 1,
                        group_count,
                        if comment_count == 1 { "" } else { "s" },
                    ));
                }
                if let Some(pb) = &main_pb {
                    pb.inc(1);
                }
                all_comments.append(&mut parsed);
            }

            if let Some(pb) = main_pb {
                pb.finish_and_clear();
            }
        } else {
            // Single LLM call
            let is_tty = std::io::stderr().is_terminal();
            let spinner = if is_tty {
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::with_template("{spinner:.cyan} {msg} ({elapsed})").unwrap(),
                );
                let file_label = if kept_diffs.len() == 1 {
                    "file"
                } else {
                    "files"
                };
                pb.set_message(format!("Reviewing {} {file_label}...", kept_diffs.len(),));
                pb.enable_steady_tick(std::time::Duration::from_millis(120));
                Some(pb)
            } else {
                None
            };

            let is_cross_file = kept_diffs.len() > 1;
            let user = prompt::build_review_prompt(
                &diff_text,
                repo_map.as_deref(),
                related_code.as_deref(),
                history_context.as_deref(),
                None,
                is_cross_file,
            );

            let messages = vec![
                ChatMessage {
                    role: Role::System,
                    content: system,
                },
                ChatMessage {
                    role: Role::User,
                    content: user,
                },
            ];

            let response = self.llm.chat(messages).await?;
            llm_calls = 1;
            all_comments = prompt::parse_review_response(&response)?;
            if let Some(pb) = spinner {
                pb.finish_with_message(format!(
                    "Reviewed → {} comment{}",
                    all_comments.len(),
                    if all_comments.len() == 1 { "" } else { "s" },
                ));
            }
        }

        let comments_generated = all_comments.len();

        // Tag comments that match custom rules
        tag_rule_matches(&mut all_comments, &self.rules);

        // 3. Deduplicate
        let (deduped, comments_deduplicated) = deduplicate(all_comments);

        // 3.5. Self-reflection pass: filter false positives
        let (reflected, comments_reflected_out) =
            if self.config.self_reflection && !deduped.is_empty() {
                let spinner = make_spinner("Self-reflecting on comments...");
                match self
                    .self_reflect(&deduped, &diff_text, &mut llm_calls)
                    .await
                {
                    Ok((kept, removed_count)) => {
                        if let Some(pb) = spinner {
                            pb.finish_with_message(format!(
                                "Self-reflection → {removed_count} filtered out"
                            ));
                        }
                        (kept, removed_count)
                    }
                    Err(e) => {
                        if let Some(pb) = spinner {
                            pb.finish_with_message("Self-reflection failed, keeping all");
                        }
                        eprintln!("warning: self-reflection failed ({e}), keeping all comments");
                        (deduped, 0)
                    }
                }
            } else {
                (deduped, 0)
            };

        // 4. Filter and sort
        let (final_comments, filtered_comments) = filter_and_sort(reflected, &self.config);
        let comments_filtered = filtered_comments.len();

        if std::io::stderr().is_terminal() {
            eprintln!(
                "✓ Done. {} comments ({} filtered, {} deduped, {} reflected out)",
                final_comments.len(),
                comments_filtered,
                comments_deduplicated,
                comments_reflected_out,
            );
        }

        // 5. Generate summary if there are comments
        let summary = if !final_comments.is_empty() {
            let spinner = make_spinner("Generating summary...");
            let summary_messages = vec![
                ChatMessage {
                    role: Role::System,
                    content: "You are a code review summarizer. Be concise.".into(),
                },
                ChatMessage {
                    role: Role::User,
                    content: prompt::build_summary_prompt(&final_comments, &diff_text),
                },
            ];
            match self.llm.chat(summary_messages).await {
                Ok(text) => {
                    llm_calls += 1;
                    if let Some(pb) = spinner {
                        pb.finish_with_message("Summary generated");
                    }
                    Some(text.trim().to_string())
                }
                Err(_) => {
                    if let Some(pb) = spinner {
                        pb.finish_with_message("Summary generation failed");
                    }
                    None
                }
            }
        } else {
            None
        };

        Ok(ReviewResult {
            comments: final_comments,
            filtered_comments,
            summary,
            stats: ReviewStats {
                files_reviewed,
                files_skipped,
                total_hunks,
                comments_generated,
                comments_filtered,
                comments_deduplicated,
                comments_reflected_out,
                skipped_files,
                model_used: self.llm.model().to_string(),
                llm_calls,
                file_groups,
            },
        })
    }

    /// Run self-reflection on the generated comments.
    ///
    /// Sends the comments and diff to the LLM for a second evaluation pass.
    /// Comments scoring below `self_reflection_score_threshold` are removed.
    /// Returns the surviving comments and the count of removed ones.
    async fn self_reflect(
        &self,
        comments: &[ReviewComment],
        diff_text: &str,
        llm_calls: &mut usize,
    ) -> Result<(Vec<ReviewComment>, usize), ArgusError> {
        let reflection_prompt = prompt::build_self_reflection_prompt(comments, diff_text);
        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: "You are a senior code reviewer evaluating AI-generated review comments. \
                          Be critical — only high-quality, verifiable issues should pass."
                    .into(),
            },
            ChatMessage {
                role: Role::User,
                content: reflection_prompt,
            },
        ];

        let response = self.llm.chat(messages).await?;
        *llm_calls += 1;

        let evaluations = prompt::parse_self_reflection_response(&response)?;

        // Build a score map: index -> (score, optional revised severity)
        let mut score_map: HashMap<usize, (u8, Option<Severity>)> = HashMap::new();
        for (idx, score, revised_sev) in evaluations {
            score_map.insert(idx, (score, revised_sev));
        }

        let threshold = self.config.self_reflection_score_threshold;
        let mut kept = Vec::new();
        let mut removed = 0usize;

        for (i, mut comment) in comments.iter().cloned().enumerate() {
            if let Some((score, revised_sev)) = score_map.get(&i) {
                if *score < threshold {
                    removed += 1;
                    continue;
                }
                // Apply revised severity if provided
                if let Some(sev) = revised_sev {
                    comment.severity = *sev;
                }
            }
            // If a comment wasn't evaluated (LLM missed it), keep it
            kept.push(comment);
        }

        Ok((kept, removed))
    }
}

/// Create a stderr spinner that only displays when stderr is a terminal.
fn make_spinner(message: &str) -> Option<ProgressBar> {
    if !std::io::stderr().is_terminal() {
        return None;
    }
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("{spinner:.cyan} {msg} ({elapsed})").unwrap());
    pb.set_message(message.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(120));
    Some(pb)
}

fn diffs_to_text<D: std::borrow::Borrow<FileDiff>>(diffs: &[D]) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    for diff in diffs {
        let diff = diff.borrow();
        let _ = writeln!(text, "--- a/{}", diff.old_path.display());
        let _ = writeln!(text, "+++ b/{}", diff.new_path.display());
        for hunk in &diff.hunks {
            let _ = writeln!(
                text,
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            );
            text.push_str(&hunk.content);
        }
    }
    text
}

fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Group related diffs by parent directory, splitting groups that exceed
/// `max_tokens`.
///
/// Files sharing a parent directory are reviewed together so the LLM can
/// catch cross-file issues. Groups that would exceed the token budget are
/// split into smaller sub-groups.
fn group_related_diffs<'a>(diffs: &'a [FileDiff], max_tokens: usize) -> Vec<Vec<&'a FileDiff>> {
    use std::path::PathBuf;

    let mut dir_groups: HashMap<PathBuf, Vec<&'a FileDiff>> = HashMap::new();
    for diff in diffs {
        let dir = Path::new(&diff.new_path)
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        dir_groups.entry(dir).or_default().push(diff);
    }

    let mut result = Vec::new();
    for (_dir, files) in dir_groups {
        let mut current_group: Vec<&FileDiff> = Vec::new();
        let mut current_tokens: usize = 0;
        for file in files {
            let file_tokens = estimate_tokens(&diffs_to_text(std::slice::from_ref(file)));
            if current_tokens + file_tokens > max_tokens && !current_group.is_empty() {
                result.push(current_group);
                current_group = Vec::new();
                current_tokens = 0;
            }
            current_group.push(file);
            current_tokens += file_tokens;
        }
        if !current_group.is_empty() {
            result.push(current_group);
        }
    }
    result
}

/// Build a human-readable label for a group of diffs.
///
/// Single-file groups show the filename. Multi-file groups sharing a
/// directory show the directory path. Mixed groups show the first few
/// filenames joined by commas.
fn group_display_name<D: std::borrow::Borrow<FileDiff>>(group: &[D]) -> String {
    if group.len() == 1 {
        let path = &group[0].borrow().new_path;
        return path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
    }

    let paths: Vec<&Path> = group
        .iter()
        .map(|d| d.borrow().new_path.as_path())
        .collect();

    if let Some(common) = common_directory(&paths) {
        let display = common.to_string_lossy();
        if display.is_empty() {
            // Root-level files, fall through to filename list
        } else {
            return format!("{display}/");
        }
    }

    let names: Vec<String> = group
        .iter()
        .take(3)
        .map(|d| {
            d.borrow()
                .new_path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    if group.len() > 3 {
        format!("{}, ...", names.join(", "))
    } else {
        names.join(", ")
    }
}

/// Find the common parent directory of a set of paths.
///
/// Returns `None` if paths share no common directory (including when the
/// common prefix is the empty path, i.e. root-level files).
fn common_directory<'a>(paths: &'a [&'a Path]) -> Option<&'a Path> {
    let first = paths.first()?;
    let mut common = first.parent()?;
    for path in &paths[1..] {
        while !path.starts_with(common) {
            common = common.parent()?;
        }
    }
    // Empty string means root — treat as "no common directory"
    if common.as_os_str().is_empty() {
        return None;
    }
    Some(common)
}

fn deduplicate(comments: Vec<ReviewComment>) -> (Vec<ReviewComment>, usize) {
    let before = comments.len();
    let mut seen: Vec<ReviewComment> = Vec::new();

    for comment in comments {
        let mut is_dup = false;
        for existing in &mut seen {
            if existing.file_path == comment.file_path
                && existing.line == comment.line
                && existing.message == comment.message
            {
                // Keep the higher confidence one
                if comment.confidence > existing.confidence {
                    existing.confidence = comment.confidence;
                }
                is_dup = true;
                break;
            }
        }
        if !is_dup {
            seen.push(comment);
        }
    }

    let deduped_count = before - seen.len();
    (seen, deduped_count)
}

fn filter_and_sort(
    comments: Vec<ReviewComment>,
    config: &ReviewConfig,
) -> (Vec<ReviewComment>, Vec<FilteredComment>) {
    let mut kept: Vec<ReviewComment> = Vec::new();
    let mut filtered: Vec<FilteredComment> = Vec::new();

    for comment in comments {
        if comment.confidence < config.min_confidence {
            filtered.push(FilteredComment {
                comment,
                reason: "below confidence threshold".into(),
            });
            continue;
        }
        if !config.severity_filter.contains(&comment.severity) {
            let sev = format!("{:?}", comment.severity).to_lowercase();
            filtered.push(FilteredComment {
                comment,
                reason: format!("{sev}-level excluded"),
            });
            continue;
        }
        kept.push(comment);
    }

    kept.sort_by_key(|c| severity_rank(c.severity));

    if kept.len() > config.max_comments {
        let truncated = kept.split_off(config.max_comments);
        for comment in truncated {
            filtered.push(FilteredComment {
                comment,
                reason: "exceeded max comment limit".into(),
            });
        }
    }

    (kept, filtered)
}

fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Bug => 0,
        Severity::Warning => 1,
        Severity::Suggestion => 2,
        Severity::Info => 3,
    }
}

/// Tag comments that reference a custom rule by name.
///
/// Checks if any rule name appears in the comment's message
/// and sets the `rule` field on matching comments.
fn tag_rule_matches(comments: &mut [ReviewComment], rules: &[Rule]) {
    for comment in comments.iter_mut() {
        for rule in rules {
            if comment.message.contains(&rule.name) {
                comment.rule = Some(rule.name.clone());
                break;
            }
        }
    }
}

/// Build related code context from the search index for the given diffs.
///
/// For each file in the diff, performs a keyword search for its entity names.
/// Returns the top 3 results formatted for inclusion in the review prompt.
fn build_related_code_context(diffs: &[FileDiff], index_path: &std::path::Path) -> Option<String> {
    let index = match argus_codelens::store::CodeIndex::open(index_path) {
        Ok(idx) => idx,
        Err(_) => return None,
    };

    let mut context_parts = Vec::new();

    for diff in diffs {
        let file_name = diff
            .new_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if file_name.is_empty() {
            continue;
        }

        let results = match index.keyword_search(file_name, 3) {
            Ok(r) => r,
            Err(_) => continue,
        };

        for hit in &results {
            // Don't include the file being reviewed as "related code"
            if hit.chunk.file_path == diff.new_path {
                continue;
            }
            context_parts.push(format!(
                "// Related: {} ({}:{})\n{}",
                hit.chunk.entity_name,
                hit.chunk.file_path.display(),
                hit.chunk.start_line,
                hit.chunk.content,
            ));
        }
    }

    if context_parts.is_empty() {
        return None;
    }

    // Limit total context size
    let mut output = String::new();
    for part in context_parts.iter().take(3) {
        if output.len() + part.len() > 4000 {
            break;
        }
        output.push_str(part);
        output.push_str("\n\n");
    }

    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}

/// Build git history context for files in the diff.
///
/// Mines recent history and identifies hotspots, coupling, and knowledge silos
/// for the changed files.
fn build_history_context(diffs: &[FileDiff], repo_path: &Path) -> Option<String> {
    let options = argus_gitpulse::mining::MiningOptions::default();
    let commits = match argus_gitpulse::mining::mine_history(repo_path, &options) {
        Ok(c) if !c.is_empty() => c,
        _ => return None,
    };

    let hotspots = argus_gitpulse::hotspots::detect_hotspots(repo_path, &commits).ok()?;
    let coupling = argus_gitpulse::coupling::detect_coupling(&commits, 0.3, 3).ok()?;
    let ownership = argus_gitpulse::ownership::analyze_ownership(&commits).ok()?;

    // Collect paths of changed files
    let changed_files: std::collections::HashSet<String> = diffs
        .iter()
        .map(|d| d.new_path.to_string_lossy().to_string())
        .collect();

    let mut lines = Vec::new();

    // Hotspot info for changed files
    for h in &hotspots {
        if changed_files.contains(&h.path) {
            lines.push(format!(
                "- {}: {} revisions in {} months, {} authors, {}",
                h.path,
                h.revisions,
                options.since_days / 30,
                h.authors,
                if h.score >= 0.7 {
                    format!("HOTSPOT (score: {:.2})", h.score)
                } else {
                    format!("score: {:.2}", h.score)
                },
            ));
        }
    }

    // Coupling info for changed files
    for pair in &coupling {
        let a_changed = changed_files.contains(&pair.file_a);
        let b_changed = changed_files.contains(&pair.file_b);
        if a_changed || b_changed {
            lines.push(format!(
                "- {} is temporally coupled with {} (coupling: {:.2}, {} co-changes)",
                pair.file_a, pair.file_b, pair.coupling_degree, pair.co_changes,
            ));
        }
    }

    // Ownership info for changed files
    for file in &ownership.files {
        if changed_files.contains(&file.path) && file.is_knowledge_silo {
            let Some(dominant) = file.authors.first() else {
                continue;
            };
            lines.push(format!(
                "- {}: knowledge silo (single author: {}, {:.0}% of commits)",
                file.path,
                dominant.email,
                dominant.ratio * 100.0,
            ));
        }
    }

    if lines.is_empty() {
        return None;
    }

    Some(lines.join("\n"))
}

impl fmt::Display for ReviewResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Review Results")?;
        writeln!(f, "==============")?;
        writeln!(
            f,
            "Model: {} | Files: {} (skipped: {}) | Hunks: {} | Comments: {} (filtered: {}, deduped: {}, reflected: {}) | LLM calls: {}\n",
            self.stats.model_used,
            self.stats.files_reviewed,
            self.stats.files_skipped,
            self.stats.total_hunks,
            self.comments.len(),
            self.stats.comments_filtered,
            self.stats.comments_deduplicated,
            self.stats.comments_reflected_out,
            self.stats.llm_calls,
        )?;

        if let Some(summary) = &self.summary {
            writeln!(f, "Summary: {summary}\n")?;
        }

        if !self.stats.skipped_files.is_empty() {
            writeln!(f, "Skipped files:")?;
            for sf in &self.stats.skipped_files {
                writeln!(f, "  {} ({})", sf.path.display(), sf.reason)?;
            }
            writeln!(f)?;
        }

        if self.comments.is_empty() {
            writeln!(f, "No issues found.")?;
        } else {
            for c in &self.comments {
                let label = match c.severity {
                    Severity::Bug => "BUG",
                    Severity::Warning => "WARNING",
                    Severity::Suggestion => "SUGGESTION",
                    Severity::Info => "INFO",
                };
                if let Some(rule) = &c.rule {
                    writeln!(
                        f,
                        "[{label}] {}:{} (confidence: {:.0}%, rule: {rule})",
                        c.file_path.display(),
                        c.line,
                        c.confidence,
                    )?;
                } else {
                    writeln!(
                        f,
                        "[{label}] {}:{} (confidence: {:.0}%)",
                        c.file_path.display(),
                        c.line,
                        c.confidence,
                    )?;
                }
                writeln!(f, "  {}", c.message)?;
                if let Some(s) = &c.suggestion {
                    writeln!(f, "  Suggestion: {s}")?;
                }
                if let Some(patch) = &c.patch {
                    writeln!(f, "  Patch:")?;
                    for line in patch.lines() {
                        writeln!(f, "    {line}")?;
                    }
                }
                writeln!(f)?;
            }
        }

        Ok(())
    }
}

impl ReviewResult {
    /// Render the review result as markdown.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_review::pipeline::{ReviewResult, ReviewStats};
    ///
    /// let result = ReviewResult {
    ///     comments: vec![],
    ///     filtered_comments: vec![],
    ///     summary: None,
    ///     stats: ReviewStats {
    ///         files_reviewed: 0,
    ///         files_skipped: 0,
    ///         total_hunks: 0,
    ///         comments_generated: 0,
    ///         comments_filtered: 0,
    ///         comments_deduplicated: 0,
    ///         comments_reflected_out: 0,
    ///         skipped_files: vec![],
    ///         model_used: "gpt-4o".into(),
    ///         llm_calls: 0,
    ///         file_groups: vec![],
    ///     },
    /// };
    /// let md = result.to_markdown();
    /// assert!(md.contains("# Review Results"));
    /// ```
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Review Results\n\n");
        out.push_str(&format!(
            "**Model:** {} | **Files:** {} (skipped: {}) | **Hunks:** {} | **Comments:** {} (filtered: {}, deduped: {}, reflected: {}) | **LLM calls:** {}\n\n",
            self.stats.model_used,
            self.stats.files_reviewed,
            self.stats.files_skipped,
            self.stats.total_hunks,
            self.comments.len(),
            self.stats.comments_filtered,
            self.stats.comments_deduplicated,
            self.stats.comments_reflected_out,
            self.stats.llm_calls,
        ));

        if let Some(summary) = &self.summary {
            out.push_str(&format!("> {summary}\n\n"));
        }

        if self.comments.is_empty() {
            out.push_str("No issues found.\n");
        } else {
            for c in &self.comments {
                let emoji = match c.severity {
                    Severity::Bug => "\u{1f41b}",
                    Severity::Warning => "\u{26a0}\u{fe0f}",
                    Severity::Suggestion => "\u{1f4a1}",
                    Severity::Info => "\u{2139}\u{fe0f}",
                };
                let label = match c.severity {
                    Severity::Bug => "Bug",
                    Severity::Warning => "Warning",
                    Severity::Suggestion => "Suggestion",
                    Severity::Info => "Info",
                };
                if let Some(rule) = &c.rule {
                    out.push_str(&format!(
                        "## {emoji} {label} — `{}:{}` ({:.0}%, rule: {rule})\n\n",
                        c.file_path.display(),
                        c.line,
                        c.confidence,
                    ));
                } else {
                    out.push_str(&format!(
                        "## {emoji} {label} — `{}:{}` ({:.0}%)\n\n",
                        c.file_path.display(),
                        c.line,
                        c.confidence,
                    ));
                }
                out.push_str(&format!("{}\n\n", c.message));
                if let Some(s) = &c.suggestion {
                    out.push_str(&format!("> **Suggestion:** {s}\n\n"));
                }
                if let Some(patch) = &c.patch {
                    out.push_str(&format!("```\n{patch}\n```\n\n"));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_comments() -> Vec<ReviewComment> {
        vec![
            ReviewComment {
                file_path: PathBuf::from("a.rs"),
                line: 1,
                severity: Severity::Info,
                message: "info comment".into(),
                confidence: 95.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
            ReviewComment {
                file_path: PathBuf::from("b.rs"),
                line: 10,
                severity: Severity::Bug,
                message: "real bug".into(),
                confidence: 98.0,
                suggestion: Some("fix it".into()),
                patch: None,
                rule: None,
            },
            ReviewComment {
                file_path: PathBuf::from("c.rs"),
                line: 20,
                severity: Severity::Warning,
                message: "potential issue".into(),
                confidence: 85.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
            ReviewComment {
                file_path: PathBuf::from("d.rs"),
                line: 30,
                severity: Severity::Bug,
                message: "low confidence bug".into(),
                confidence: 50.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
        ]
    }

    #[test]
    fn filter_removes_low_confidence() {
        let config = ReviewConfig {
            min_confidence: 90.0,
            severity_filter: vec![Severity::Bug, Severity::Warning, Severity::Info],
            max_comments: 10,
            ..ReviewConfig::default()
        };
        let (kept, filtered) = filter_and_sort(make_comments(), &config);
        // c.rs (85%) and d.rs (50%) should be removed
        assert_eq!(kept.len(), 2);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|f| f.reason.contains("confidence")));
    }

    #[test]
    fn filter_removes_non_matching_severity() {
        let config = ReviewConfig {
            min_confidence: 0.0,
            severity_filter: vec![Severity::Bug, Severity::Warning],
            max_comments: 10,
            ..ReviewConfig::default()
        };
        let (kept, filtered) = filter_and_sort(make_comments(), &config);
        // Info comment should be removed
        for c in &kept {
            assert!(c.severity == Severity::Bug || c.severity == Severity::Warning);
        }
        assert!(filtered.iter().any(|f| f.reason.contains("excluded")));
    }

    #[test]
    fn sort_by_severity_bug_first() {
        let config = ReviewConfig {
            min_confidence: 0.0,
            severity_filter: vec![
                Severity::Bug,
                Severity::Warning,
                Severity::Suggestion,
                Severity::Info,
            ],
            max_comments: 10,
            ..ReviewConfig::default()
        };
        let (kept, _) = filter_and_sort(make_comments(), &config);
        assert!(kept.len() >= 2);
        // Bugs should come before warnings/info
        assert_eq!(kept[0].severity, Severity::Bug);
    }

    #[test]
    fn truncate_to_max_comments() {
        let config = ReviewConfig {
            min_confidence: 0.0,
            severity_filter: vec![
                Severity::Bug,
                Severity::Warning,
                Severity::Suggestion,
                Severity::Info,
            ],
            max_comments: 2,
            ..ReviewConfig::default()
        };
        let (kept, filtered) = filter_and_sort(make_comments(), &config);
        assert_eq!(kept.len(), 2);
        assert!(filtered
            .iter()
            .any(|f| f.reason.contains("max comment limit")));
    }

    #[test]
    fn deduplication_merges_identical_comments() {
        let comments = vec![
            ReviewComment {
                file_path: PathBuf::from("a.rs"),
                line: 10,
                severity: Severity::Bug,
                message: "null deref".into(),
                confidence: 85.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
            ReviewComment {
                file_path: PathBuf::from("a.rs"),
                line: 10,
                severity: Severity::Bug,
                message: "null deref".into(),
                confidence: 95.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
            ReviewComment {
                file_path: PathBuf::from("b.rs"),
                line: 20,
                severity: Severity::Warning,
                message: "different issue".into(),
                confidence: 90.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
        ];
        let (deduped, count) = deduplicate(comments);
        assert_eq!(deduped.len(), 2);
        assert_eq!(count, 1);
        // Should keep the higher confidence
        let a_comment = deduped
            .iter()
            .find(|c| c.file_path == PathBuf::from("a.rs"))
            .unwrap();
        assert!((a_comment.confidence - 95.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_tokens_rough_calc() {
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    #[test]
    fn display_and_markdown_output() {
        let result = ReviewResult {
            comments: vec![ReviewComment {
                file_path: PathBuf::from("test.rs"),
                line: 5,
                severity: Severity::Bug,
                message: "test bug".into(),
                confidence: 99.0,
                suggestion: Some("fix it".into()),
                patch: None,
                rule: None,
            }],
            filtered_comments: vec![],
            summary: None,
            stats: ReviewStats {
                files_reviewed: 1,
                files_skipped: 0,
                total_hunks: 1,
                comments_generated: 1,
                comments_filtered: 0,
                comments_deduplicated: 0,
                comments_reflected_out: 0,
                skipped_files: vec![],
                model_used: "test".into(),
                llm_calls: 1,
                file_groups: vec![],
            },
        };
        let text = format!("{result}");
        assert!(text.contains("[BUG]"));
        assert!(text.contains("test.rs:5"));

        let md = result.to_markdown();
        assert!(md.contains("# Review Results"));
        assert!(md.contains("Bug"));
    }

    fn make_file_diff(path: &str, content: &str) -> FileDiff {
        use argus_core::{ChangeType, DiffHunk};
        FileDiff {
            old_path: PathBuf::from(path),
            new_path: PathBuf::from(path),
            hunks: vec![DiffHunk {
                file_path: PathBuf::from(path),
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                content: content.into(),
                change_type: ChangeType::Add,
            }],
            is_new_file: true,
            is_deleted_file: false,
            is_rename: false,
        }
    }

    #[test]
    fn group_same_directory_files_together() {
        let diffs = vec![
            make_file_diff("src/pipeline.rs", "+a\n"),
            make_file_diff("src/prompt.rs", "+b\n"),
            make_file_diff("tests/integration.rs", "+c\n"),
        ];
        let groups = group_related_diffs(&diffs, 100_000);
        // Two directories: src/ and tests/
        assert_eq!(groups.len(), 2);

        let mut group_sizes: Vec<usize> = groups.iter().map(|g| g.len()).collect();
        group_sizes.sort();
        assert_eq!(group_sizes, vec![1, 2]);
    }

    #[test]
    fn group_different_directories_separate() {
        let diffs = vec![
            make_file_diff("crates/core/src/lib.rs", "+a\n"),
            make_file_diff("crates/review/src/lib.rs", "+b\n"),
            make_file_diff("crates/mcp/src/lib.rs", "+c\n"),
        ];
        let groups = group_related_diffs(&diffs, 100_000);
        assert_eq!(groups.len(), 3);
        for group in &groups {
            assert_eq!(group.len(), 1);
        }
    }

    #[test]
    fn group_splits_on_token_limit() {
        // Each file has ~25 chars → ~6 tokens. With a limit of 10 tokens,
        // two same-directory files should be split into separate groups.
        let diffs = vec![
            make_file_diff("src/a.rs", &"+".repeat(50)),
            make_file_diff("src/b.rs", &"+".repeat(50)),
        ];
        let groups = group_related_diffs(&diffs, 10);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn group_single_file_no_grouping() {
        let diffs = vec![make_file_diff("src/lib.rs", "+a\n")];
        let groups = group_related_diffs(&diffs, 100_000);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 1);
    }

    #[test]
    fn group_root_directory_files() {
        let diffs = vec![
            make_file_diff("README.md", "+a\n"),
            make_file_diff("Cargo.toml", "+b\n"),
        ];
        let groups = group_related_diffs(&diffs, 100_000);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
    }

    #[test]
    fn tag_rule_matches_sets_rule_field() {
        let rules = vec![Rule {
            name: "no-unwrap".into(),
            severity: "warning".into(),
            description: "Don't use unwrap".into(),
        }];
        let mut comments = vec![
            ReviewComment {
                file_path: PathBuf::from("a.rs"),
                line: 10,
                severity: Severity::Warning,
                message: "Using .unwrap() violates no-unwrap rule".into(),
                confidence: 95.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
            ReviewComment {
                file_path: PathBuf::from("b.rs"),
                line: 20,
                severity: Severity::Bug,
                message: "Null pointer dereference".into(),
                confidence: 90.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
        ];
        tag_rule_matches(&mut comments, &rules);
        assert_eq!(comments[0].rule.as_deref(), Some("no-unwrap"));
        assert!(comments[1].rule.is_none());
    }

    #[test]
    fn group_display_name_single_file() {
        let diffs = vec![make_file_diff(
            "crates/argus-review/src/pipeline.rs",
            "+a\n",
        )];
        let refs: Vec<&FileDiff> = diffs.iter().collect();
        assert_eq!(group_display_name(&refs), "pipeline.rs");
    }

    #[test]
    fn group_display_name_same_directory() {
        let diffs = vec![
            make_file_diff("src/pipeline.rs", "+a\n"),
            make_file_diff("src/prompt.rs", "+b\n"),
        ];
        let refs: Vec<&FileDiff> = diffs.iter().collect();
        assert_eq!(group_display_name(&refs), "src/");
    }

    #[test]
    fn group_display_name_mixed_directories() {
        let diffs = vec![
            make_file_diff("README.md", "+a\n"),
            make_file_diff("Cargo.toml", "+b\n"),
        ];
        let refs: Vec<&FileDiff> = diffs.iter().collect();
        // Root-level files have no common directory — shows filenames
        let name = group_display_name(&refs);
        assert!(name.contains("README.md"));
        assert!(name.contains("Cargo.toml"));
    }

    #[test]
    fn common_directory_same_parent() {
        let a = Path::new("src/a.rs");
        let b = Path::new("src/b.rs");
        let paths = [a, b];
        let result = common_directory(&paths);
        assert_eq!(result, Some(Path::new("src")));
    }

    #[test]
    fn common_directory_nested() {
        let a = Path::new("crates/core/src/lib.rs");
        let b = Path::new("crates/core/src/types.rs");
        let paths = [a, b];
        let result = common_directory(&paths);
        assert_eq!(result, Some(Path::new("crates/core/src")));
    }

    #[test]
    fn common_directory_no_common() {
        let a = Path::new("README.md");
        let b = Path::new("Cargo.toml");
        let paths = [a, b];
        let result = common_directory(&paths);
        assert!(result.is_none());
    }

    #[test]
    fn common_directory_divergent_trees() {
        let a = Path::new("crates/core/src/lib.rs");
        let b = Path::new("crates/review/src/lib.rs");
        let paths = [a, b];
        let result = common_directory(&paths);
        assert_eq!(result, Some(Path::new("crates")));
    }

    #[test]
    fn common_directory_empty_input() {
        let result = common_directory(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn display_shows_summary_when_present() {
        let result = ReviewResult {
            comments: vec![ReviewComment {
                file_path: PathBuf::from("test.rs"),
                line: 5,
                severity: Severity::Bug,
                message: "test bug".into(),
                confidence: 99.0,
                suggestion: None,
                patch: None,
                rule: None,
            }],
            filtered_comments: vec![],
            summary: Some("High risk. Key issue is a null dereference.".into()),
            stats: ReviewStats {
                files_reviewed: 1,
                files_skipped: 0,
                total_hunks: 1,
                comments_generated: 1,
                comments_filtered: 0,
                comments_deduplicated: 0,
                comments_reflected_out: 0,
                skipped_files: vec![],
                model_used: "test".into(),
                llm_calls: 2,
                file_groups: vec![],
            },
        };
        let text = format!("{result}");
        assert!(text.contains("Summary: High risk. Key issue is a null dereference."));
    }

    #[test]
    fn display_omits_summary_when_none() {
        let result = ReviewResult {
            comments: vec![],
            filtered_comments: vec![],
            summary: None,
            stats: ReviewStats {
                files_reviewed: 0,
                files_skipped: 0,
                total_hunks: 0,
                comments_generated: 0,
                comments_filtered: 0,
                comments_deduplicated: 0,
                comments_reflected_out: 0,
                skipped_files: vec![],
                model_used: "test".into(),
                llm_calls: 0,
                file_groups: vec![],
            },
        };
        let text = format!("{result}");
        assert!(!text.contains("Summary:"));
    }

    #[test]
    fn markdown_includes_summary_blockquote() {
        let result = ReviewResult {
            comments: vec![ReviewComment {
                file_path: PathBuf::from("test.rs"),
                line: 5,
                severity: Severity::Bug,
                message: "test bug".into(),
                confidence: 99.0,
                suggestion: None,
                patch: None,
                rule: None,
            }],
            filtered_comments: vec![],
            summary: Some("Medium risk due to missing error handling.".into()),
            stats: ReviewStats {
                files_reviewed: 1,
                files_skipped: 0,
                total_hunks: 1,
                comments_generated: 1,
                comments_filtered: 0,
                comments_deduplicated: 0,
                comments_reflected_out: 0,
                skipped_files: vec![],
                model_used: "test".into(),
                llm_calls: 2,
                file_groups: vec![],
            },
        };
        let md = result.to_markdown();
        assert!(md.contains("> Medium risk due to missing error handling."));
    }

    #[test]
    fn display_shows_patch_when_present() {
        let result = ReviewResult {
            comments: vec![ReviewComment {
                file_path: PathBuf::from("test.rs"),
                line: 5,
                severity: Severity::Bug,
                message: "test bug".into(),
                confidence: 99.0,
                suggestion: Some("fix it".into()),
                patch: Some("let x = safe_call();\nuse(x);".into()),
                rule: None,
            }],
            filtered_comments: vec![],
            summary: None,
            stats: ReviewStats {
                files_reviewed: 1,
                files_skipped: 0,
                total_hunks: 1,
                comments_generated: 1,
                comments_filtered: 0,
                comments_deduplicated: 0,
                comments_reflected_out: 0,
                skipped_files: vec![],
                model_used: "test".into(),
                llm_calls: 1,
                file_groups: vec![],
            },
        };
        let text = format!("{result}");
        assert!(text.contains("Patch:"));
        assert!(text.contains("    let x = safe_call();"));
        assert!(text.contains("    use(x);"));
    }

    #[test]
    fn markdown_shows_patch_code_block() {
        let result = ReviewResult {
            comments: vec![ReviewComment {
                file_path: PathBuf::from("test.rs"),
                line: 5,
                severity: Severity::Bug,
                message: "test bug".into(),
                confidence: 99.0,
                suggestion: None,
                patch: Some("let x = safe_call();".into()),
                rule: None,
            }],
            filtered_comments: vec![],
            summary: None,
            stats: ReviewStats {
                files_reviewed: 1,
                files_skipped: 0,
                total_hunks: 1,
                comments_generated: 1,
                comments_filtered: 0,
                comments_deduplicated: 0,
                comments_reflected_out: 0,
                skipped_files: vec![],
                model_used: "test".into(),
                llm_calls: 1,
                file_groups: vec![],
            },
        };
        let md = result.to_markdown();
        assert!(md.contains("```\nlet x = safe_call();\n```"));
    }
}
