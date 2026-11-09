//! Throughput on large logs: `cargo bench`.

use ci_doctor::{InputFormatArg, diagnose};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

/// A 100k-line log that looks like a long CI job: mostly noise, a few failures.
fn synthetic_log(lines: usize) -> String {
    let noise = [
        "2026-11-09T10:00:00.0000000Z Downloading https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz",
        "tests/test_models.py::test_case PASSED                                  [ 12%]",
        "   Compiling serde v1.0.229",
        "[INFO] Building jar: /work/target/app-1.0.jar",
        "ok   github.com/acme/api/internal/store  0.412s",
    ];
    let mut out = String::with_capacity(lines * 80);
    for i in 0..lines {
        let line = match i % 10_000 {
            1_234 => "npm ERR! code ERESOLVE",
            5_678 => "FAILED tests/test_api.py::test_users - AssertionError: 1 != 2",
            9_999 => "##[error]The operation was canceled.",
            _ => noise[i % noise.len()],
        };
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn bench_text(c: &mut Criterion) {
    let log = synthetic_log(100_000);
    let mut group = c.benchmark_group("text_log");
    group.throughput(Throughput::Bytes(log.len() as u64));
    group.bench_function("100k_lines", |b| {
        b.iter(|| diagnose(black_box(&log), InputFormatArg::Text).unwrap())
    });
    group.finish();
}

fn bench_junit(c: &mut Criterion) {
    let mut xml = String::from("<?xml version=\"1.0\"?><testsuite>");
    for i in 0..5_000 {
        if i % 500 == 0 {
            xml.push_str(&format!(
                "<testcase classname=\"pkg.Suite\" name=\"t{i}\"><failure message=\"expected: &lt;1&gt; but was: &lt;2&gt;\">trace</failure></testcase>"
            ));
        } else {
            xml.push_str(&format!(
                "<testcase classname=\"pkg.Suite\" name=\"t{i}\"/>"
            ));
        }
    }
    xml.push_str("</testsuite>");

    let mut group = c.benchmark_group("junit");
    group.throughput(Throughput::Bytes(xml.len() as u64));
    group.bench_function("5k_testcases", |b| {
        b.iter(|| diagnose(black_box(&xml), InputFormatArg::Junit).unwrap())
    });
    group.finish();
}

criterion_group!(benches, bench_text, bench_junit);
criterion_main!(benches);
