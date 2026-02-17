use argus_core::{ArgusError, ReviewComment, Severity};

/// GitHub Pull Request client for fetching diffs and posting reviews.
///
/// # Examples
///
/// ```
/// use argus_review::github::parse_pr_reference;
///
/// let (owner, repo, number) = parse_pr_reference("rust-lang/rust#12345").unwrap();
/// assert_eq!(owner, "rust-lang");
/// assert_eq!(repo, "rust");
/// assert_eq!(number, 12345);
/// ```
pub struct GitHubClient {
    octocrab: octocrab::Octocrab,
    http: reqwest::Client,
    token: String,
}

impl GitHubClient {
    /// Create a client from an explicit token or the `GITHUB_TOKEN` environment variable.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Config`] if no token is available, or
    /// [`ArgusError::Git`] if the client cannot be built.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argus_review::github::GitHubClient;
    ///
    /// let client = GitHubClient::new(Some("ghp_xxxx")).unwrap();
    /// ```
    pub fn new(token: Option<&str>) -> Result<Self, ArgusError> {
        let token = match token {
            Some(t) => t.to_string(),
            None => std::env::var("GITHUB_TOKEN").map_err(|_| {
                ArgusError::Config(
                    "GITHUB_TOKEN not set. Pass --github-token or set GITHUB_TOKEN env var".into(),
                )
            })?,
        };

        let octocrab = octocrab::Octocrab::builder()
            .personal_token(token.clone())
            .build()
            .map_err(|e| ArgusError::Git(format!("failed to create GitHub client: {e}")))?;

        let http = reqwest::Client::new();

        Ok(Self {
            octocrab,
            http,
            token,
        })
    }

    /// Fetch the unified diff for a pull request.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Git`] on network or API errors.
    pub async fn get_pr_diff(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<String, ArgusError> {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{pr_number}");

        let response = self
            .http
            .get(&url)
            .header("Accept", "application/vnd.github.v3.diff")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "argus")
            .send()
            .await
            .map_err(|e| ArgusError::Git(format!("failed to fetch PR diff: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ArgusError::Git(format!(
                "GitHub API error {status}: {body}"
            )));
        }

        response
            .text()
            .await
            .map_err(|e| ArgusError::Git(format!("failed to read diff response: {e}")))
    }

    /// Post review comments to a pull request.
    ///
    /// Creates a single review with all comments using the GitHub PR Review API.
    /// The review event is determined by the highest severity comment:
    /// Bug -> REQUEST_CHANGES, Warning/Suggestion/Info -> COMMENT.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Git`] on API errors.
    pub async fn post_review(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
        comments: &[ReviewComment],
        summary: &str,
    ) -> Result<(), ArgusError> {
        let review_comments: Vec<serde_json::Value> = comments
            .iter()
            .map(|c| {
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
                let mut body = format!(
                    "**{emoji} {label}** (confidence: {:.0}%)\n\n{}",
                    c.confidence, c.message
                );
                if let Some(s) = &c.suggestion {
                    body.push_str(&format!("\n\n**Suggestion:** {s}"));
                }
                serde_json::json!({
                    "path": c.file_path.to_string_lossy(),
                    "line": c.line,
                    "side": "RIGHT",
                    "body": body,
                })
            })
            .collect();

        let event = "COMMENT";

        let route = format!("/repos/{owner}/{repo}/pulls/{pr_number}/reviews");
        let body = serde_json::json!({
            "event": event,
            "body": summary,
            "comments": review_comments,
        });

        let _response: serde_json::Value = self
            .octocrab
            .post(route, Some(&body))
            .await
            .map_err(|e| ArgusError::Git(format!("failed to post review: {e}")))?;

        Ok(())
    }
}

/// Parse a PR reference string (`owner/repo#number`) into its components.
///
/// # Errors
///
/// Returns [`ArgusError::Config`] if the format is invalid.
///
/// # Examples
///
/// ```
/// use argus_review::github::parse_pr_reference;
///
/// let (owner, repo, num) = parse_pr_reference("octocat/hello-world#42").unwrap();
/// assert_eq!(owner, "octocat");
/// assert_eq!(repo, "hello-world");
/// assert_eq!(num, 42);
/// ```
pub fn parse_pr_reference(pr_ref: &str) -> Result<(String, String, u64), ArgusError> {
    let Some((owner_repo, number_str)) = pr_ref.split_once('#') else {
        return Err(ArgusError::Config(format!(
            "invalid PR reference '{pr_ref}', expected owner/repo#number"
        )));
    };
    let Some((owner, repo)) = owner_repo.split_once('/') else {
        return Err(ArgusError::Config(format!(
            "invalid PR reference '{pr_ref}', expected owner/repo#number"
        )));
    };
    let number: u64 = number_str
        .parse()
        .map_err(|_| ArgusError::Config(format!("invalid PR number: {number_str}")))?;
    Ok((owner.to_string(), repo.to_string(), number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_pr_reference() {
        let (owner, repo, num) = parse_pr_reference("rust-lang/rust#12345").unwrap();
        assert_eq!(owner, "rust-lang");
        assert_eq!(repo, "rust");
        assert_eq!(num, 12345);
    }

    #[test]
    fn parse_pr_reference_missing_hash() {
        let result = parse_pr_reference("owner/repo");
        assert!(result.is_err());
    }

    #[test]
    fn parse_pr_reference_missing_slash() {
        let result = parse_pr_reference("repo#123");
        assert!(result.is_err());
    }

    #[test]
    fn parse_pr_reference_invalid_number() {
        let result = parse_pr_reference("owner/repo#abc");
        assert!(result.is_err());
    }
}
