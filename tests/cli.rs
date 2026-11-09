//! End-to-end tests that run the compiled binary on realistic CI output.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ci-doctor"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.unwrap_or("").as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn json(args: &[&str]) -> (i32, serde_json::Value) {
    let out = run(args, None);
    let value = serde_json::from_slice(&out.stdout).expect("valid JSON on stdout");
    (out.status.code().unwrap(), value)
}

fn categories(v: &serde_json::Value) -> Vec<String> {
    v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["category"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn npm_dependency_conflict_is_one_incident() {
    let path = fixture("gha_npm_eresolve.log");
    let (code, v) = json(&["diagnose", "--format", "json", path.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert_eq!(v["input_format"], "text");
    assert_eq!(categories(&v), ["dependency-conflict"]);
    assert_eq!(v["findings"][0]["line"], 5);
}

#[test]
fn job_timeout_is_detected_but_timeout_config_is_not() {
    let path = fixture("gha_timeout.log");
    let (_, v) = json(&["diagnose", "--format", "json", path.to_str().unwrap()]);
    assert_eq!(categories(&v), ["timeout"]);
    assert_eq!(
        v["findings"][0]["line"], 6,
        "line 2 (timeout-minutes) is configuration, not a failure"
    );
}

#[test]
fn pytest_rerun_is_flaky_and_real_failure_is_named() {
    let path = fixture("pytest_rerun.log");
    let (_, v) = json(&["diagnose", "--format", "json", path.to_str().unwrap()]);
    let tests: Vec<(&str, &str)> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| Some((f["test"].as_str()?, f["category"].as_str()?)))
        .collect();
    assert!(tests.contains(&("tests/test_api.py::test_list_users", "flaky-test")));
    assert!(tests.contains(&(
        "tests/test_billing.py::test_invoice_total",
        "assertion-failure"
    )));
    // The plugin banner and the "1 rerun" summary line must not add findings.
    assert_eq!(v["total_failures"], 2);
}

#[test]
fn cargo_test_failure_is_named() {
    let path = fixture("cargo_mixed.log");
    let (_, v) = json(&["diagnose", "--format", "json", path.to_str().unwrap()]);
    assert_eq!(v["findings"][0]["test"], "parser::tests::empty_input");
    assert_eq!(v["findings"][0]["category"], "assertion-failure");
}

#[test]
fn surefire_report_is_auto_detected_and_classified() {
    let path = fixture("surefire.xml");
    let (code, v) = json(&["diagnose", "--format", "json", path.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert_eq!(v["input_format"], "junit");
    assert_eq!(v["total_failures"], 3);
    let by_test: Vec<(String, String)> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["test"].as_str().unwrap().to_string(),
                f["category"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        by_test,
        [
            (
                "com.acme.shop.CartTest.appliesCoupon".into(),
                "assertion-failure".into()
            ),
            (
                "com.acme.shop.CartTest.loadsPrices".into(),
                "network-error".into()
            ),
            (
                "com.acme.shop.CartTest.concurrentCheckout".into(),
                "flaky-test".into()
            ),
        ]
    );
    assert_eq!(
        v["findings"][0]["message"],
        "expected: <90.0> but was: <100.0>"
    );
}

#[test]
fn reads_stdin_and_exits_zero_when_clean() {
    let out = run(&["diagnose", "-"], Some("Run tests\n3 passed in 0.2s\n"));
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("no failures found"));
}

#[test]
fn text_output_names_category_and_next_step() {
    let out = run(&["diagnose"], Some("Segmentation fault (core dumped)\n"));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("[crash] (line 1)"));
    assert!(text.contains("next step: Rebuild with -g -fsanitize=address"));
}

#[test]
fn invalid_utf8_does_not_crash() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ci-doctor"))
        .arg("diagnose")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\xff\xfe broken bytes\nconnection refused\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn malformed_junit_exits_two() {
    let out = run(
        &["diagnose", "--input-format", "junit"],
        Some("<testsuite><testcase></testsuite>"),
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid JUnit XML"));
}

#[test]
fn missing_file_exits_two() {
    let out = run(&["diagnose", "/definitely/not/here.log"], None);
    assert_eq!(out.status.code(), Some(2));
}
