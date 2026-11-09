//! Rule-based classification of a single log line or failure message.
//!
//! All patterns are compiled once into a `RegexSet`, so each line is matched
//! against every rule in a single pass. Rules are ordered by specificity and
//! the first matching rule wins: "ETIMEDOUT" is a network error, not a
//! timeout, and "exit code 137" is memory, not a generic crash.

use crate::model::Category;
use regex::{Regex, RegexSet};
use std::sync::LazyLock;

struct Rule {
    category: Category,
    pattern: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        category: Category::OutOfMemory,
        pattern: r"(?i)(out of memory|\bOOMKilled\b|\bMemoryError\b|java\.lang\.OutOfMemoryError|JavaScript heap out of memory|exit code:? 137\b|Cannot allocate memory)",
    },
    Rule {
        category: Category::Crash,
        pattern: r"(?i)(segmentation fault|\bSIGSEGV\b|\bSIGABRT\b|core dumped|ERROR: AddressSanitizer|runtime error: .*(overflow|out of bounds|null pointer)|fatal runtime error)",
    },
    Rule {
        category: Category::NetworkError,
        pattern: r"(?i)(\bECONNREFUSED\b|\bECONNRESET\b|\bETIMEDOUT\b|\bENOTFOUND\b|\bEAI_AGAIN\b|could not resolve host|connection refused|connection reset by peer|TLS handshake (timeout|failed)|429 Too Many Requests|50[234] (Bad Gateway|Service Unavailable|Gateway Time-?out)|temporary failure in name resolution)",
    },
    Rule {
        category: Category::Timeout,
        pattern: r"(?i)(\btimed out\b|\btimeout expired\b|deadline exceeded|exceeded the maximum execution time|the operation was canceled|\bTimeoutError\b|timeout of \d+ ?ms exceeded|test timed out after)",
    },
    Rule {
        category: Category::DependencyConflict,
        pattern: r"(?i)(\bERESOLVE\b|could not resolve dependenc|ResolutionImpossible|conflicting dependencies|version solving failed|no matching distribution found|could not find a version that satisfies|failed to select a version for|\bModuleNotFoundError\b|cannot find module '|unable to resolve dependency tree|peer dep(endency)? conflict)",
    },
    Rule {
        category: Category::PermissionDenied,
        pattern: r"(?i)(permission denied|\bEACCES\b|\bEPERM\b|403 Forbidden|resource not accessible by integration|authentication failed)",
    },
    Rule {
        category: Category::CompileError,
        pattern: r"(error\[E\d{4}\]|error TS\d+:|\bSyntaxError:|\bIndentationError:|cannot find symbol|COMPILATION ERROR|: error: |could not compile `)",
    },
    Rule {
        category: Category::Lint,
        pattern: r"(?i)(\d+ problems? \(\d+ errors?|would reformat|Found \d+ errors?\.|clippy::|^\S+:\d+:\d+: [A-Z]\d{3} )",
    },
    Rule {
        category: Category::FlakyTest,
        // RERUN and the Surefire element names are matched case-sensitively so that
        // banners like "plugins: rerunfailures-14.0" and summaries like "1 rerun" don't count.
        pattern: r"(\bRERUN\b|\bflakyFailure\b|\brerunFailure\b|(?i:passed on retry|\bis flaky\b|\bflaky test\b|retry(ing)? (attempt )?\d+ ?(/|of) ?\d+))",
    },
    Rule {
        category: Category::AssertionFailure,
        pattern: r"(\bAssertionError\b|assertion failed|assertion `.+` failed|\bexpected:? .+ (but was|got|received)\b|Expected:|^FAILED \S+::|--- FAIL: |^test \S+ \.\.\. FAILED|panicked at|org\.opentest4j\.AssertionFailedError|junit\.framework\.AssertionFailedError)",
    },
];

static SET: LazyLock<RegexSet> =
    LazyLock::new(|| RegexSet::new(RULES.iter().map(|r| r.pattern)).expect("valid rule patterns"));

/// Test-name extractors for common runners: pytest (summary and verbose), Go, cargo test.
static TEST_NAME: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^FAILED (\S+::\S+)",
        r"^(\S+::\S+) (?:RERUN|FAILED|ERROR)\b",
        r"--- FAIL: (\S+)",
        r"^test (\S+) \.\.\. FAILED",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("valid test-name pattern"))
    .collect()
});

/// Classify one line or message; `None` when it does not look like a failure.
pub fn classify(text: &str) -> Option<Category> {
    // Iteration order of matches() follows rule order, so the first is the most specific.
    SET.matches(text).iter().next().map(|i| RULES[i].category)
}

/// Pull a test identifier out of a runner's failure line, if it has one.
pub fn test_name(line: &str) -> Option<String> {
    TEST_NAME
        .iter()
        .find_map(|re| re.captures(line))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_failures() {
        let cases = [
            ("npm ERR! code ERESOLVE", Category::DependencyConflict),
            (
                "ERROR: No matching distribution found for torch==9.9",
                Category::DependencyConflict,
            ),
            (
                "error: failed to select a version for `serde`.",
                Category::DependencyConflict,
            ),
            (
                "connect ECONNREFUSED 127.0.0.1:5432",
                Category::NetworkError,
            ),
            ("##[error]The operation was canceled.", Category::Timeout),
            (
                "FATAL ERROR: Reached heap limit Allocation failed - JavaScript heap out of memory",
                Category::OutOfMemory,
            ),
            (
                "Process completed with exit code 137.",
                Category::OutOfMemory,
            ),
            ("Segmentation fault (core dumped)", Category::Crash),
            (
                "==12==ERROR: AddressSanitizer: heap-buffer-overflow on address",
                Category::Crash,
            ),
            ("error[E0308]: mismatched types", Category::CompileError),
            (
                "src/app.ts(3,7): error TS2322: Type 'string' is not assignable",
                Category::CompileError,
            ),
            (
                "remote: Permission denied to bot.",
                Category::PermissionDenied,
            ),
            ("tests/test_api.py::test_users RERUN", Category::FlakyTest),
            (
                "FAILED tests/test_api.py::test_users - AssertionError: 1 != 2",
                Category::AssertionFailure,
            ),
            ("--- FAIL: TestParse (0.00s)", Category::AssertionFailure),
            (
                "test parser::tests::empty ... FAILED",
                Category::AssertionFailure,
            ),
            ("would reformat src/main.py", Category::Lint),
        ];
        for (line, expected) in cases {
            assert_eq!(classify(line), Some(expected), "line: {line}");
        }
    }

    #[test]
    fn ignores_ordinary_lines() {
        for line in [
            "timeout-minutes: 10",
            "Run actions/checkout@v4",
            "collected 120 items",
            "test result: ok. 12 passed; 0 failed",
            "Setting connection timeout to 30s",
            "Expected behaviour is documented in README",
            "plugins: rerunfailures-14.0, flaky-3.8.1",
            "==================== 1 failed, 84 passed, 1 rerun in 4.21s ====",
        ] {
            assert_eq!(classify(line), None, "line: {line}");
        }
    }

    #[test]
    fn more_specific_rule_wins() {
        // Mentions both a timeout and a network error code: the code is more precise.
        assert_eq!(
            classify("request timed out: ETIMEDOUT"),
            Some(Category::NetworkError)
        );
        // A retried test that failed an assertion is flaky first.
        assert_eq!(
            classify("RERUN tests/test_x.py::test_y AssertionError"),
            Some(Category::FlakyTest)
        );
    }

    #[test]
    fn extracts_test_names() {
        assert_eq!(
            test_name("FAILED tests/test_api.py::test_users - assert 1 == 2").as_deref(),
            Some("tests/test_api.py::test_users")
        );
        assert_eq!(
            test_name("--- FAIL: TestParse/empty (0.00s)").as_deref(),
            Some("TestParse/empty")
        );
        assert_eq!(
            test_name("test parser::tests::empty ... FAILED").as_deref(),
            Some("parser::tests::empty")
        );
        assert_eq!(
            test_name("tests/test_api.py::test_list_users RERUN      [ 31%]").as_deref(),
            Some("tests/test_api.py::test_list_users")
        );
        assert_eq!(test_name("npm ERR! code ERESOLVE"), None);
    }
}
