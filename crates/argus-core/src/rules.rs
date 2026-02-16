use crate::config::Rule;

/// Parse natural language rules from a markdown string.
///
/// Supports list items with optional severity tags and name prefixes.
///
/// Format examples:
/// - `[bug] no-unwrap: Do not use unwrap`
/// - `[warning] Check error handling`
/// - `Ensure variable names are descriptive`
pub fn parse_rules_markdown(content: &str) -> Vec<Rule> {
    let mut rules = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // Skip empty lines and headers
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Strip list markers
        let clean_line = if let Some(stripped) = line.strip_prefix("- ") {
            stripped
        } else if let Some(stripped) = line.strip_prefix("* ") {
            stripped
        } else {
            line
        }
        .trim();

        if clean_line.is_empty() {
            continue;
        }

        // Parse severity "[severity]"
        let (severity, rest) = if clean_line.starts_with('[') {
            if let Some(end) = clean_line.find(']') {
                let sev_str = clean_line[1..end].to_lowercase();
                let rest = clean_line[end + 1..].trim();
                (sev_str, rest)
            } else {
                ("warning".to_string(), clean_line)
            }
        } else {
            ("warning".to_string(), clean_line)
        };

        // Parse name/description split "Name: Description"
        // We assume it's a name if it's short-ish and followed by a colon
        let (name, description) = if let Some(colon_idx) = rest.find(':') {
            let possible_name = rest[..colon_idx].trim();
            // simple heuristic: if name is short and doesn't contain too many spaces
            if possible_name.len() < 40 && possible_name.chars().filter(|c| *c == ' ').count() < 4 {
                (possible_name.to_string(), rest[colon_idx + 1..].trim().to_string())
            } else {
                (generate_slug(rest), rest.to_string())
            }
        } else {
            (generate_slug(rest), rest.to_string())
        };

        if !description.is_empty() {
            rules.push(Rule {
                name,
                severity,
                description,
            });
        }
    }

    rules
}

fn generate_slug(text: &str) -> String {
    text.split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_explicit_severity_and_name() {
        let input = "- [bug] no-unwrap: Do not use unwrap()";
        let rules = parse_rules_markdown(input);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].severity, "bug");
        assert_eq!(rules[0].name, "no-unwrap");
        assert_eq!(rules[0].description, "Do not use unwrap()");
    }

    #[test]
    fn parse_explicit_severity_only() {
        let input = "- [info] Just a note about style";
        let rules = parse_rules_markdown(input);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].severity, "info");
        assert_eq!(rules[0].name, "just-a-note-about");
        assert_eq!(rules[0].description, "Just a note about style");
    }

    #[test]
    fn parse_implicit_defaults() {
        let input = "Always use camelCase";
        let rules = parse_rules_markdown(input);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].severity, "warning");
        assert_eq!(rules[0].name, "always-use-camelcase");
        assert_eq!(rules[0].description, "Always use camelCase");
    }

    #[test]
    fn parse_multiple_lines_with_comments() {
        let input = r#"
# My Rules

- [bug] security: No raw SQL
- prefer-iterators: Use iterators where possible

* [warning] Check for TODOs
"#;
        let rules = parse_rules_markdown(input);
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].name, "security");
        assert_eq!(rules[1].name, "prefer-iterators");
        assert_eq!(rules[2].name, "check-for-todos");
    }
}
