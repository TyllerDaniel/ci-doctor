//! Human-readable rendering.

use crate::model::Diagnosis;
use std::fmt::Write;

pub fn render_text(d: &Diagnosis) -> String {
    let mut out = String::new();
    if d.findings.is_empty() {
        let _ = writeln!(
            out,
            "ci-doctor: no failures found ({} input)",
            d.input_format
        );
        return out;
    }
    let _ = writeln!(
        out,
        "ci-doctor: {} failure(s) in {} input\n",
        d.total_failures, d.input_format
    );
    for f in &d.findings {
        let location = match (&f.test, f.line) {
            (Some(test), _) => format!(" {test}"),
            (None, Some(line)) => format!(" (line {line})"),
            (None, None) => String::new(),
        };
        let _ = writeln!(out, "[{}]{location}", f.category);
        let _ = writeln!(out, "  {}", f.message);
        let _ = writeln!(out, "  evidence:  {}", f.evidence);
        let _ = writeln!(out, "  next step: {}\n", f.suggestion);
    }
    let summary: Vec<String> = d.summary.iter().map(|(k, v)| format!("{k} {v}")).collect();
    let _ = writeln!(out, "by category: {}", summary.join(", "));
    out
}
