//! Diagnose a raw CI log (GitHub Actions, GitLab, Jenkins, or plain runner output).

use crate::classify::{classify, test_name};
use crate::model::{Category, Finding};
use std::collections::HashMap;

/// Matches of the same category within this many lines are one incident
/// (an npm error prints ten `npm ERR!` lines for a single conflict).
const MERGE_WINDOW: usize = 15;

/// Strip GitHub Actions timestamps and annotation prefixes so rules see the real message.
fn normalize(line: &str) -> &str {
    let mut s = line.trim_end_matches('\r');
    // "2026-11-09T14:03:11.1234567Z message"
    if s.len() > 29
        && s.as_bytes().get(10) == Some(&b'T')
        && s[..4].bytes().all(|b| b.is_ascii_digit())
        && let Some(pos) = s.find("Z ")
    {
        s = &s[pos + 2..];
    }
    for prefix in ["##[error]", "##[warning]", "::error::"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
        }
    }
    s
}

pub fn diagnose_text(log: &str) -> Vec<Finding> {
    let mut findings: Vec<Finding> = Vec::new();
    // Last line at which each untitled category was reported, for merging.
    let mut last_seen: HashMap<Category, usize> = HashMap::new();
    // Each named test is reported once, at its most specific category.
    let mut by_test: HashMap<String, usize> = HashMap::new();

    for (idx, raw) in log.lines().enumerate() {
        let line_no = idx + 1;
        let line = normalize(raw);
        let Some(category) = classify(line) else {
            continue;
        };

        if let Some(test) = test_name(line) {
            match by_test.get(&test) {
                // A test that was retried before failing is flaky, whatever the final error.
                Some(&i) if findings[i].category == Category::FlakyTest => {}
                Some(&i) => {
                    if category == Category::FlakyTest {
                        findings[i] = Finding::new(
                            category,
                            Some(test.clone()),
                            message_for(category),
                            line,
                            Some(line_no),
                        );
                    }
                }
                None => {
                    by_test.insert(test.clone(), findings.len());
                    findings.push(Finding::new(
                        category,
                        Some(test),
                        message_for(category),
                        line,
                        Some(line_no),
                    ));
                }
            }
            continue;
        }

        if let Some(&prev) = last_seen.get(&category)
            && line_no - prev <= MERGE_WINDOW
        {
            last_seen.insert(category, line_no);
            continue;
        }
        last_seen.insert(category, line_no);
        findings.push(Finding::new(
            category,
            None,
            message_for(category),
            line,
            Some(line_no),
        ));
    }

    // A named test failure already explains a generic assertion line next to it.
    if findings.iter().any(|f| f.test.is_some()) {
        findings.retain(|f| f.test.is_some() || f.category != Category::AssertionFailure);
    }
    findings
}

fn message_for(category: Category) -> &'static str {
    match category {
        Category::OutOfMemory => "process ran out of memory",
        Category::Crash => "process crashed",
        Category::NetworkError => "network request failed",
        Category::Timeout => "step exceeded its time limit",
        Category::DependencyConflict => "dependencies could not be resolved",
        Category::PermissionDenied => "permission denied",
        Category::CompileError => "build failed to compile",
        Category::Lint => "lint or formatting check failed",
        Category::FlakyTest => "test was retried, so it is likely flaky",
        Category::AssertionFailure => "test assertion failed",
        Category::RuntimeError => "unhandled runtime error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_github_actions_prefixes() {
        assert_eq!(
            normalize("2026-11-09T14:03:11.1234567Z ##[error]Process completed"),
            "Process completed"
        );
        assert_eq!(normalize("::error::boom"), "boom");
        assert_eq!(normalize("plain line"), "plain line");
    }

    #[test]
    fn merges_repeated_lines_into_one_incident() {
        let log = "npm ERR! code ERESOLVE\nnpm ERR! ERESOLVE unable to resolve dependency tree\nnpm ERR! peer dep conflict\n";
        let findings = diagnose_text(log);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, Category::DependencyConflict);
        assert_eq!(findings[0].line, Some(1));
    }

    #[test]
    fn separate_incidents_far_apart_are_kept() {
        let mut log = String::from("connection refused\n");
        log.push_str(&"ok\n".repeat(40));
        log.push_str("connection refused\n");
        assert_eq!(diagnose_text(&log).len(), 2);
    }

    #[test]
    fn one_finding_per_named_test() {
        let log = "FAILED tests/a.py::test_x - AssertionError\nFAILED tests/a.py::test_x - AssertionError\nFAILED tests/a.py::test_y - AssertionError\n";
        let findings = diagnose_text(log);
        let tests: Vec<_> = findings.iter().map(|f| f.test.clone().unwrap()).collect();
        assert_eq!(tests, ["tests/a.py::test_x", "tests/a.py::test_y"]);
    }

    #[test]
    fn clean_log_has_no_findings() {
        assert!(diagnose_text("Run tests\ncollected 3 items\n3 passed in 0.1s\n").is_empty());
    }
}
