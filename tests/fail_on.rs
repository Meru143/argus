use argus_core::Severity;

#[test]
fn fail_on_exits_zero_when_no_matching_severity() {
    // Simulate: only Suggestion-level findings, threshold is Bug
    let comments = vec![Severity::Suggestion, Severity::Info];
    let threshold = Severity::Bug;

    let has_findings = comments.iter().any(|s| s.meets_threshold(threshold));
    assert!(!has_findings, "should not fail when no bug-level findings");
}

#[test]
fn fail_on_exits_one_when_matching_severity_found() {
    // Simulate: Bug finding present, threshold is Warning
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
