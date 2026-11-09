//! Output types. The JSON shape is a stable contract used by devenv-mcp.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    OutOfMemory,
    Crash,
    NetworkError,
    Timeout,
    DependencyConflict,
    PermissionDenied,
    CompileError,
    Lint,
    FlakyTest,
    AssertionFailure,
    RuntimeError,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::OutOfMemory => "out-of-memory",
            Category::Crash => "crash",
            Category::NetworkError => "network-error",
            Category::Timeout => "timeout",
            Category::DependencyConflict => "dependency-conflict",
            Category::PermissionDenied => "permission-denied",
            Category::CompileError => "compile-error",
            Category::Lint => "lint",
            Category::FlakyTest => "flaky-test",
            Category::AssertionFailure => "assertion-failure",
            Category::RuntimeError => "runtime-error",
        }
    }

    /// What a developer should try first for this kind of failure.
    pub fn suggestion(self) -> &'static str {
        match self {
            Category::OutOfMemory => {
                "Measure peak memory of the failing step; reduce parallelism or batch size, or raise the runner's memory limit."
            }
            Category::Crash => {
                "Rebuild with -g -fsanitize=address (or run under gdb) to get a stack trace for the faulting access."
            }
            Category::NetworkError => {
                "Check whether it is transient (retry once) or a real outage; cache dependencies and avoid live network calls in tests."
            }
            Category::Timeout => {
                "Time each step locally to find the slow one and look for hangs on network, locks or prompts before raising the limit."
            }
            Category::DependencyConflict => {
                "Compare the lockfile with the declared version ranges, pin or upgrade the conflicting package, then regenerate the lockfile."
            }
            Category::PermissionDenied => {
                "Check token scopes, workflow permissions and file modes; CI usually runs as a different user than your machine."
            }
            Category::CompileError => {
                "Fix the first compiler error first; later errors are often caused by it."
            }
            Category::Lint => {
                "Run the same linter or formatter version locally and apply its fixes."
            }
            Category::FlakyTest => {
                "Reproduce by running the test in a loop and in random order; look for shared state, timing and ordering assumptions. Don't just add retries."
            }
            Category::AssertionFailure => {
                "Compare expected and actual values, then reproduce with only this test before changing code."
            }
            Category::RuntimeError => {
                "Read the stack trace from the bottom up to the first frame in your own code."
            }
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub category: Category,
    /// Test identifier when the input names one.
    pub test: Option<String>,
    pub message: String,
    /// The line or message that triggered the classification.
    pub evidence: String,
    /// 1-based line number for text logs.
    pub line: Option<usize>,
    pub suggestion: String,
}

impl Finding {
    pub fn new(
        category: Category,
        test: Option<String>,
        message: impl Into<String>,
        evidence: &str,
        line: Option<usize>,
    ) -> Self {
        Finding {
            category,
            test,
            message: message.into(),
            evidence: clip(evidence.trim(), 240),
            line,
            suggestion: category.suggestion().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InputFormat {
    Junit,
    Text,
}

impl fmt::Display for InputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InputFormat::Junit => "junit",
            InputFormat::Text => "text",
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnosis {
    pub input_format: InputFormat,
    pub total_failures: usize,
    /// Count of findings per category, sorted by category name.
    pub summary: BTreeMap<String, usize>,
    pub findings: Vec<Finding>,
}

impl Diagnosis {
    pub fn new(input_format: InputFormat, findings: Vec<Finding>) -> Self {
        let mut summary = BTreeMap::new();
        for f in &findings {
            *summary.entry(f.category.as_str().to_string()).or_insert(0) += 1;
        }
        Diagnosis {
            input_format,
            total_failures: findings.len(),
            summary,
            findings,
        }
    }
}

/// Truncate to at most `max` characters on a char boundary.
pub fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((idx, _)) => format!("{}...", &s[..idx]),
        None => s.to_string(),
    }
}
