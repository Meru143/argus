use argus_core::Severity;

#[test]
fn meets_threshold_is_false_when_no_severity_matches() {
    // In-memory threshold check: only Suggestion/Info findings, threshold is Bug
    let comments = vec![Severity::Suggestion, Severity::Info];
    let threshold = Severity::Bug;

    let has_findings = comments.iter().any(|s| s.meets_threshold(threshold));
    assert!(!has_findings, "should not fail when no bug-level findings");
}

#[test]
fn meets_threshold_is_true_when_higher_severity_is_present() {
    // In-memory threshold check: Bug finding present, threshold is Warning
    let comments = vec![Severity::Bug, Severity::Suggestion];
    let threshold = Severity::Warning;

    let has_findings = comments.iter().any(|s| s.meets_threshold(threshold));
    assert!(has_findings, "should fail when bug meets warning threshold");
}

#[test]
fn fail_on_warning_catches_bugs_and_warnings() {
    let threshold = Severity::Warning;

    assert!(Severity::Bug.meets_threshold(threshold));
    assert!(Severity::Warning.meets_threshold(threshold));
    assert!(!Severity::Suggestion.meets_threshold(threshold));
    assert!(!Severity::Info.meets_threshold(threshold));
}
