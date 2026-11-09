//! Diagnose JUnit XML reports (pytest --junitxml, Maven Surefire, Gradle, Jest, Go via gotestsum).

use crate::classify::classify;
use crate::model::{Category, Finding};
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Passed,
    Skipped,
    /// `<failure>`: an assertion did not hold.
    Failed,
    /// `<error>`: the test crashed or raised unexpectedly.
    Errored,
}

#[derive(Debug, Default)]
struct Case {
    id: String,
    outcome: Option<Outcome>,
    /// Surefire writes `<flakyFailure>` when a test failed and then passed on rerun.
    flaky_marker: bool,
    message: String,
    body: String,
}

impl Case {
    fn outcome(&self) -> Outcome {
        self.outcome.unwrap_or(Outcome::Passed)
    }
}

#[derive(Debug)]
pub struct JunitError(pub String);

impl std::fmt::Display for JunitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid JUnit XML: {}", self.0)
    }
}

impl std::error::Error for JunitError {}

fn attr(e: &BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| {
            a.normalized_value(XmlVersion::default())
                .ok()
                .map(|v| v.into_owned())
        })
}

/// quick-xml reports `&lt;` and friends as separate events; turn them back into text.
fn resolve_ref(r: &BytesRef<'_>) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    match &**r {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    }
    .to_string()
}

fn case_id(e: &BytesStart<'_>) -> String {
    let name = attr(e, "name").unwrap_or_else(|| "<unnamed>".to_string());
    match attr(e, "classname") {
        Some(class) if !class.is_empty() => format!("{class}.{name}"),
        _ => name,
    }
}

fn parse_cases(xml: &str) -> Result<Vec<Case>, JunitError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut cases = Vec::new();
    let mut current: Option<Case> = None;
    // Name of the child element whose text we are collecting (failure, error, ...).
    let mut capturing: Option<String> = None;

    loop {
        let event = reader.read_event().map_err(|e| JunitError(e.to_string()))?;
        match event {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == "testcase" => {
                if let Some(done) = current.take() {
                    cases.push(done); // previous <testcase/> was self-closing
                }
                current = Some(Case {
                    id: case_id(&e),
                    ..Default::default()
                });
            }
            Event::Start(e) | Event::Empty(e) => {
                let Some(case) = current.as_mut() else {
                    continue;
                };
                let local = e.local_name();
                let tag = local.as_ref();
                let outcome = match tag {
                    "failure" => Some(Outcome::Failed),
                    "error" => Some(Outcome::Errored),
                    "skipped" => Some(Outcome::Skipped),
                    _ => None,
                };
                if matches!(tag, "flakyFailure" | "flakyError") {
                    case.flaky_marker = true;
                }
                if let Some(o) = outcome {
                    // A real failure outranks a skip; keep the first failure's details.
                    if case.outcome.is_none() || case.outcome == Some(Outcome::Skipped) {
                        case.outcome = Some(o);
                        case.message = attr(&e, "message").unwrap_or_default();
                    }
                }
                if matches!(tag, "failure" | "error") {
                    capturing = Some(tag.to_string());
                }
            }
            Event::Text(t) => {
                if let (Some(case), Some(_)) = (current.as_mut(), &capturing) {
                    case.body.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if let (Some(case), Some(_)) = (current.as_mut(), &capturing) {
                    case.body.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) => {
                if let (Some(case), Some(_)) = (current.as_mut(), &capturing) {
                    case.body.push_str(&resolve_ref(&r));
                }
            }
            Event::End(e) => {
                let local = e.local_name();
                if capturing.as_deref() == Some(local.as_ref()) {
                    capturing = None;
                }
                if local.as_ref() == "testcase"
                    && let Some(done) = current.take()
                {
                    cases.push(done);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if let Some(done) = current.take() {
        cases.push(done);
    }
    Ok(cases)
}

pub fn diagnose_junit(xml: &str) -> Result<Vec<Finding>, JunitError> {
    let cases = parse_cases(xml)?;

    // Group by test id, keeping first-seen order: a test that appears several
    // times is a rerun, and mixed outcomes across reruns mean it is flaky.
    let mut order: Vec<String> = Vec::new();
    let mut runs: BTreeMap<String, Vec<&Case>> = BTreeMap::new();
    for case in &cases {
        if !runs.contains_key(&case.id) {
            order.push(case.id.clone());
        }
        runs.entry(case.id.clone()).or_default().push(case);
    }

    let mut findings = Vec::new();
    for id in order {
        let attempts = &runs[&id];
        let failed: Vec<&Case> = attempts
            .iter()
            .copied()
            .filter(|c| matches!(c.outcome(), Outcome::Failed | Outcome::Errored))
            .collect();
        let passed = attempts.iter().any(|c| c.outcome() == Outcome::Passed);
        let marked_flaky = attempts.iter().any(|c| c.flaky_marker);

        if (!failed.is_empty() && passed) || (marked_flaky && failed.is_empty()) {
            let evidence = failed
                .first()
                .map(|c| first_line(c))
                .unwrap_or("failed, then passed on rerun");
            findings.push(Finding::new(
                Category::FlakyTest,
                Some(id),
                format!(
                    "failed {} of {} run(s) and passed on another",
                    failed.len().max(1),
                    attempts.len()
                ),
                evidence,
                None,
            ));
            continue;
        }

        let Some(last) = failed.last() else { continue };
        let text = format!("{}\n{}", last.message, last.body);
        let category =
            text.lines()
                .find_map(classify)
                .unwrap_or(if last.outcome() == Outcome::Failed {
                    Category::AssertionFailure
                } else {
                    Category::RuntimeError
                });
        let message = if last.message.is_empty() {
            "test failed".to_string()
        } else {
            crate::model::clip(&last.message, 200)
        };
        findings.push(Finding::new(
            category,
            Some(id),
            message,
            first_line(last),
            None,
        ));
    }
    Ok(findings)
}

/// The most useful single line: the message, else the first non-empty body line.
fn first_line(case: &Case) -> &str {
    if !case.message.trim().is_empty() {
        return case.message.lines().next().unwrap_or("");
    }
    case.body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_failures_with_class_and_name() {
        let xml = r#"<testsuite>
            <testcase classname="tests.test_math" name="test_add"/>
            <testcase classname="tests.test_math" name="test_div">
                <failure message="AssertionError: assert 2 == 3">tests/test_math.py:9: AssertionError</failure>
            </testcase>
        </testsuite>"#;
        let findings = diagnose_junit(xml).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].test.as_deref(),
            Some("tests.test_math.test_div")
        );
        assert_eq!(findings[0].category, Category::AssertionFailure);
    }

    #[test]
    fn classifies_from_the_body_and_decodes_entities() {
        let xml = r#"<testsuite><testcase name="test_db">
            <error message="">Traceback...
psycopg.OperationalError: connect ECONNREFUSED 127.0.0.1:5432 &amp; retry &lt;none&gt;</error>
        </testcase></testsuite>"#;
        let findings = diagnose_junit(xml).unwrap();
        assert_eq!(findings[0].category, Category::NetworkError);
        assert_eq!(findings[0].evidence, "Traceback...");
    }

    #[test]
    fn error_without_known_pattern_is_runtime_error() {
        let xml = r#"<testsuite><testcase name="t"><error message="KeyError: 'id'"/></testcase></testsuite>"#;
        assert_eq!(
            diagnose_junit(xml).unwrap()[0].category,
            Category::RuntimeError
        );
    }

    #[test]
    fn mixed_outcomes_across_reruns_are_flaky() {
        let xml = r#"<testsuites><testsuite>
            <testcase classname="api" name="test_list"><failure message="assert 200 == 503"/></testcase>
            <testcase classname="api" name="test_list"/>
        </testsuite></testsuites>"#;
        let findings = diagnose_junit(xml).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, Category::FlakyTest);
        assert_eq!(
            findings[0].message,
            "failed 1 of 2 run(s) and passed on another"
        );
    }

    #[test]
    fn surefire_flaky_failure_marker_is_flaky() {
        let xml = r#"<testsuite><testcase classname="com.acme.CartTest" name="totals">
            <flakyFailure message="expected: &lt;10&gt; but was: &lt;9&gt;" type="org.opentest4j.AssertionFailedError"/>
        </testcase></testsuite>"#;
        let findings = diagnose_junit(xml).unwrap();
        assert_eq!(findings[0].category, Category::FlakyTest);
        assert_eq!(
            findings[0].test.as_deref(),
            Some("com.acme.CartTest.totals")
        );
    }

    #[test]
    fn skipped_and_passing_tests_are_ignored() {
        let xml = r#"<testsuite><testcase name="a"><skipped/></testcase><testcase name="b"/></testsuite>"#;
        assert!(diagnose_junit(xml).unwrap().is_empty());
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(diagnose_junit("<testsuite><testcase></testsuite>").is_err());
    }
}
