//! ci-doctor: classify CI failures from raw logs or JUnit XML and suggest a next step.
//!
//! ```
//! use ci_doctor::{diagnose, InputFormatArg};
//!
//! let d = diagnose("npm ERR! code ERESOLVE", InputFormatArg::Auto).unwrap();
//! assert_eq!(d.total_failures, 1);
//! assert_eq!(d.findings[0].category.as_str(), "dependency-conflict");
//! ```

pub mod classify;
pub mod junit;
pub mod model;
pub mod report;
pub mod textlog;

pub use model::{Category, Diagnosis, Finding, InputFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormatArg {
    Auto,
    Junit,
    Text,
}

/// JUnit reports start with an XML declaration or a <testsuite(s)> root.
pub fn detect_format(input: &str) -> InputFormat {
    let head = input.trim_start();
    let head = head.strip_prefix('\u{feff}').unwrap_or(head);
    if head.starts_with("<?xml") || head.starts_with("<testsuite") {
        InputFormat::Junit
    } else {
        InputFormat::Text
    }
}

pub fn diagnose(input: &str, format: InputFormatArg) -> Result<Diagnosis, junit::JunitError> {
    let format = match format {
        InputFormatArg::Auto => detect_format(input),
        InputFormatArg::Junit => InputFormat::Junit,
        InputFormatArg::Text => InputFormat::Text,
    };
    let findings = match format {
        InputFormat::Junit => junit::diagnose_junit(input)?,
        InputFormat::Text => textlog::diagnose_text(input),
    };
    Ok(Diagnosis::new(format, findings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats() {
        assert_eq!(
            detect_format("<?xml version=\"1.0\"?><testsuite/>"),
            InputFormat::Junit
        );
        assert_eq!(detect_format("  \n<testsuites>"), InputFormat::Junit);
        assert_eq!(detect_format("Run npm test"), InputFormat::Text);
    }
}
