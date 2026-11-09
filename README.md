# ci-doctor

[![CI](https://github.com/TyllerDaniel/ci-doctor/actions/workflows/ci.yml/badge.svg)](https://github.com/TyllerDaniel/ci-doctor/actions/workflows/ci.yml)

A fast command-line tool that reads a failed CI log or JUnit XML report, says
**what kind of failure** it is, and suggests **what to try first**.

```
$ ci-doctor diagnose build.log

ci-doctor: 2 failure(s) in text input

[flaky-test] tests/test_api.py::test_list_users
  test was retried, so it is likely flaky
  evidence:  tests/test_api.py::test_list_users RERUN                  [ 31%]
  next step: Reproduce by running the test in a loop and in random order; look for
             shared state, timing and ordering assumptions. Don't just add retries.

[assertion-failure] tests/test_billing.py::test_invoice_total
  test assertion failed
  evidence:  FAILED tests/test_billing.py::test_invoice_total - assert 15.49 == 15.0
  next step: Compare expected and actual values, then reproduce with only this test.

by category: assertion-failure 1, flaky-test 1
```

It is also the CI-diagnosis backend for the
[devenv-mcp](https://github.com/TyllerDaniel/devenv-mcp) server, which exposes it
to AI agents as the `diagnose_ci` tool.

## Install

```bash
cargo install --git https://github.com/TyllerDaniel/ci-doctor
```

Requires Rust 1.88 or newer.

## Usage

```bash
ci-doctor diagnose build.log                    # text report
ci-doctor diagnose --format json build.log      # machine-readable
ci-doctor diagnose report.xml                   # JUnit XML, auto-detected
gh run view 123 --log-failed | ci-doctor diagnose   # stdin
```

| Exit code | Meaning |
| --- | --- |
| 0 | No failures found |
| 1 | Failures found |
| 2 | Bad usage, unreadable file or malformed XML |

So it can gate a pipeline step: `ci-doctor diagnose junit.xml || notify-team`.

## What it recognises

| Category | Examples |
| --- | --- |
| `out-of-memory` | `OOMKilled`, `JavaScript heap out of memory`, `exit code 137` |
| `crash` | segfaults, `SIGABRT`, AddressSanitizer reports |
| `network-error` | `ECONNREFUSED`, `ETIMEDOUT`, DNS failures, 429/502/503 |
| `timeout` | GitHub Actions job limit, `deadline exceeded`, test timeouts |
| `dependency-conflict` | npm `ERESOLVE`, pip `ResolutionImpossible`, cargo version selection |
| `permission-denied` | `EACCES`, 403, missing workflow token scopes |
| `compile-error` | `error[E0308]`, `error TS2322`, javac `cannot find symbol` |
| `lint` | ESLint, Ruff, Black, Clippy |
| `flaky-test` | pytest `RERUN`, Surefire `<flakyFailure>`, mixed pass/fail across reruns |
| `assertion-failure` | pytest, Go, cargo test, JUnit `<failure>` |
| `runtime-error` | JUnit `<error>` with no more specific match |

Test names are extracted from pytest (summary and verbose), Go, cargo test and
JUnit `classname.name`.

## How it works

* **One pass per line.** All rules are compiled into a single `RegexSet`, so
  every line is checked against every rule at once. Rules are ordered by
  specificity and the first match wins: `ETIMEDOUT` is a network error rather
  than a timeout, and a retried test is flaky even if its final failure was an
  assertion.
* **Incidents, not lines.** One npm conflict prints a dozen `npm ERR!` lines.
  Matches of the same category within 15 lines merge into one finding, and each
  named test is reported once.
* **Low false-positive rate on purpose.** `timeout-minutes: 30`, "Setting
  connection timeout to 30s", the `plugins: rerunfailures` banner and pytest's
  "1 rerun" summary are not failures, and unit tests pin that down.
* **Flaky detection from structure.** In JUnit input, a test that appears more
  than once with both passing and failing runs is flaky, as is any test with a
  Surefire `<flakyFailure>` and no final failure.
* **Robust input.** GitHub Actions timestamps and `##[error]` prefixes are
  stripped before matching, invalid UTF-8 is decoded lossily instead of
  aborting, and XML entities and CDATA are decoded.

## Performance

Measured with `cargo bench` (Criterion) on one core of the build machine:

| Input | Time | Throughput |
| --- | --- | --- |
| 100,000-line CI log | ~20 ms | ~285 MiB/s |
| JUnit XML with 5,000 test cases | ~4.5 ms | ~48 MiB/s |

## JSON output

```json
{
  "input_format": "junit",
  "total_failures": 1,
  "summary": { "network-error": 1 },
  "findings": [
    {
      "category": "network-error",
      "test": "com.acme.shop.CartTest.loadsPrices",
      "message": "Connection refused",
      "evidence": "Connection refused",
      "line": null,
      "suggestion": "Check whether it is transient (retry once) or a real outage; ..."
    }
  ]
}
```

`line` is set for text logs, `test` whenever the runner names one. The shape is
a stable contract consumed by devenv-mcp.

## Development

```bash
cargo test          # unit tests plus end-to-end tests on fixtures in tests/fixtures
cargo clippy --all-targets -- -D warnings
cargo bench
```

## License

MIT
