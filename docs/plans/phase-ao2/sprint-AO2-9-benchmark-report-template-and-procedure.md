---
phase: AO2
sprint: AO2.9
title: Benchmark report template, publication, and aggregate procedure
branch: plan/ao2-9-benchmark-report-template
integration_branch: integrate/phase-ao2
status: ready_for_review
depends_on:
  - AO2.5.4-mandatory-benchmark-snapshot-restore
  - AO2.6-admission-writer-batching-regression
  - AO2.7-m5-tcp-benchmark-parity
  - AO2.8-windows-tcp-benchmark-parity
---

# AO2.9 — Benchmark report template, publication, and aggregate procedure

## Decision and scope

AO2.9 closes the benchmark-evidence process gap. It defines one durable
single-run report contract, one immutable publication path, and one aggregate
index for every physical `just benchmark` attempt. It is documentation and
procedure work: it does not change the daemon, client, storage, transport,
TLS, benchmark algorithm, or performance threshold.

The benchmark remains an explicitly isolated operation under AO2.5.4. It must
not use the interactive account, the interactive daemon, an alternate data
root, or a benchmark-only production behavior. A result is evidence only when
its report and raw artifacts are published by the procedure below.

The report contract is normative for AO2.7 and AO2.8. Those sprint documents
retain their target-specific thresholds and safety gates; they defer to this
document for template, variable schema, path, publication, and aggregation.
They must not duplicate those details, so the contract cannot drift.

## Existing assets and implementation boundary

The repository already contains the intended sc-compose assets:

| Asset | Contract |
| --- | --- |
| `templates/benchmark-report/benchmark-run.xhtml.j2` | Single-run XHTML panel; its front matter is the variable contract. |
| `templates/benchmark-report/benchmark-report.html.j2` | Aggregate benchmark report shell. |
| `scripts/smoke/benchmark_report.py` | Validates/persists benchmark JSON and renders the two templates. |
| `.just/generate_report_index.py` | Builds the public `site/reports/index.html` from validated envelopes. |
| `Justfile` recipe `benchmark` | Isolated benchmark entry point and finalizer hook. |

The existing implementation is a starting point, not proof that AO2.9 is
complete. In particular, it currently writes a flat historical directory and
only validates `uds` and `tcp`. The implementation work derived from this
plan must extend those seams rather than introduce a second renderer or an
untracked reporting path.

## Single-run template contract

The canonical template is
`templates/benchmark-report/benchmark-run.xhtml.j2`. It is rendered with
`sc-compose render --root <repo> --file <template> --var-file <vars.json>
--output <run.xhtml>`. The producer must validate the variables before
rendering and must never interpolate untrusted values as HTML fragments.

The template front matter is the authoritative required-variable list. The
values have these types and meanings:

| Variable | Type and rule |
| --- | --- |
| `title` | Non-empty string; escaped by the template. |
| `artifact_id` | Safe opaque run identifier; `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`. |
| `generated_at` | UTC ISO-8601 timestamp with `Z`; generated once for the attempt. |
| `host_label` | Stable, safe per-computer label; never a raw hostname, path, username, or IP. |
| `transport` | One of the target values in the authoritative matrix below (`sqlite`, `uds`, `tcp`, `tcp+tls`). |
| `frames_per_connection` | Non-negative integer; `0` for a target where frames are not applicable. |
| `run_duration_s` | Non-negative number covering the measured interval only. |
| `passed` | Boolean. `false` for a failed or incomplete attempt. |
| `failure` | Escaped diagnostic string; empty when no failure occurred. |
| `cleanup_failure` | Escaped cleanup/restore diagnostic string; empty when cleanup succeeded. |
| `sample_html` | Renderer-owned, already-escaped table fragment; never caller-supplied HTML. |
| `direct_sqlite_html` | Renderer-owned, already-escaped SQLite evidence fragment; never caller-supplied HTML. |

The JSON result beside the XHTML is the source of truth. The XHTML is a
view of that immutable JSON and must contain the run identity, status, target,
tested SHA, host label, measured duration, diagnostics, and links to raw
evidence. A failed or incomplete run is a valid report with `passed: false`;
it must not be omitted or converted into a passing empty result.

## Authoritative target matrix

This matrix supersedes narrower AO2.7/AO2.8 wording. `sqlite` is a direct
storage-admission target; it is not silently substituted for a public
transport run. `uds`, `tcp`, and `tcp+tls` are public transport targets.

| Operating system | Required targets |
| --- | --- |
| macOS | `sqlite`, `uds`, `tcp`, `tcp+tls` |
| Linux | `sqlite`, `uds`, `tcp`, `tcp+tls` |
| Windows | `sqlite`, `tcp`, `tcp+tls` |

The producer schema, CLI validation, aggregate table, and acceptance tests
must all use this same matrix. Windows must reject `uds` as unsupported; it
must not report a skipped UDS case as a pass. A target that was not attempted
is represented as missing in the aggregate, not as a synthetic result.

## Immutable publication layout

Every attempt receives one `run_id` before any build or benchmark command is
started. It is a UTC timestamp in `YYYYMMDDTHHMMSSZ` form. If two attempts on
the same host collide in one second, append `-<8 lowercase hex nonce>`; the
resulting ID remains safe and immutable.

The required layout is:

```text
site/reports/benchmark/
  index.html                              # generated aggregate/final report
  <host_label>/
    <run_id>/
      result.json                         # immutable validated source
      result.envelope.json                # public-index envelope
      run.xhtml                           # sc-compose single-run report
      raw/                                # stdout, stderr, daemon logs, traces
```

`host_label` is the same safe opaque label passed to the template. The
envelope's `report_html` is the safe repository-relative path
`benchmark/<host_label>/<run_id>/run.xhtml`; it must not contain `..`, an
absolute path, a backslash, or an unvalidated host string. The envelope also
records `schema_version`, `report_type: "benchmark"`, `generated_at`, and
`host_label`. The aggregate links to the JSON, XHTML, and raw evidence using
paths relative to `site/reports`.

The implementation must extend `.just/generate_report_index.py` to discover
and validate nested benchmark envelopes and to reject a missing run directory,
missing XHTML, malformed envelope, unsafe path, or missing raw evidence. The
root `site/reports/index.html` remains the public cross-type index and must
link to `benchmark/index.html`; the benchmark aggregate is the authoritative
single report for the benchmark family. Historical reports remain immutable;
there is no pruning or overwrite policy in AO2.9.

## Mandatory lifecycle for every `just benchmark` attempt

The `benchmark` recipe (or its one bounded wrapper) owns a `try/finally`
equivalent finalizer. The finalizer runs for success, build failure, runner
failure, validation failure, timeout, cancellation, and cleanup/restore
failure. It must:

1. Allocate `run_id`, `host_label`, target, source SHA, binary hashes, and the
   per-run directory before execution.
2. Capture command lines, stdout, stderr, exit status, target, profile,
   account identity, lifecycle phase, and all raw evidence under `raw/`.
3. Emit `result.json` with `status` (`PASS`, `FAIL`, or `INCOMPLETE`) and
   diagnostics. `INCOMPLETE` is rendered with `passed: false`; it is never
   silently treated as `FAIL` or success.
4. Render `run.xhtml` with the canonical template and write the envelope.
5. Rebuild `site/reports/benchmark/index.html` and the root report index.
6. Immediately `git add` the complete run directory and generated indexes,
   create a commit identifying `run_id` and status, and push it to the
   operator's evidence branch. The push must happen before the operator starts
   another benchmark target.

If rendering, indexing, commit, or push fails, the finalizer reports a
process violation and preserves the local run directory for recovery. It
must not claim that the run was published. A missing or uncommitted run is a
documented process violation even when the measured benchmark passed.

The benchmark command is manual/agent-triggered because it requires a
dedicated physical account and host lifecycle. CI does not run physical
benchmarks or manufacture evidence. CI must run the report-schema tests and
`just reports-index --check`; if a CI job is later authorized to execute a
physical run, it must use this same finalizer and push through a bot evidence
branch/PR. No direct push to `develop` or an integration branch is required by
this procedure.

## Aggregate/final report

`site/reports/benchmark/index.html` is regenerated after every attempt from
all immutable `result.json` records. It must show, at minimum:

- current campaign status (`PASS`, `FAIL`, or `INCOMPLETE`),
- one row for every attempted target, including failed/incomplete attempts,
- missing targets from the authoritative OS matrix,
- host label, run ID, tested SHA, target/profile, and measured status,
- links to each run's JSON, XHTML, and raw evidence, and
- historical count and generation timestamp.

The aggregate must not collapse a failed historical run when a later run
passes. Campaign summaries may identify the latest candidate, but all immutable
per-run records remain discoverable. A complete campaign is `PASS` only when
all required targets for that OS are present and pass their target-specific
acceptance gates; otherwise it is `FAIL` or `INCOMPLETE` with the missing
reason shown.

## AO2.7 and AO2.8 relationship

AO2.7 remains the M5 TCP f8 throughput evidence sprint and AO2.8 remains the
Windows parity sprint. Their existing thresholds, dedicated-account safety
requirements, and target-specific evidence remain authoritative. They must
reference this document for the publication path and use the `tcp` target
record (not a generic historical HTML file). AO2.8 additionally records the
accepted AO2.7 M5 artifact and calculated floor in its run JSON. The matrix,
failure/incomplete publication rule, and aggregate/index behavior are owned
only here and are not copied into either sprint document.

## Acceptance criteria

- [ ] The implementation uses `templates/benchmark-report/benchmark-run.xhtml.j2`
      and its front matter as the validated single-run variable contract.
- [ ] The schema/producer accepts exactly the authoritative per-OS target
      matrix, including `sqlite` and `tcp+tls`, and rejects Windows UDS.
- [ ] Every success, failure, and incomplete `just benchmark` attempt emits
      immutable JSON, XHTML, envelope, and raw evidence under
      `site/reports/benchmark/<host_label>/<run_id>/`.
- [ ] The finalizer executes on build, runner, timeout, cancellation, and
      cleanup/restore failure, and records the exit status and diagnostics.
- [ ] The finalizer rebuilds `site/reports/benchmark/index.html` and the root
      index, then commits and pushes before another target is started.
- [ ] `.just/generate_report_index.py` validates nested benchmark paths and
      rejects unsafe or incomplete envelopes; `just reports-index --check` is
      green for the published tree.
- [ ] The aggregate preserves all immutable historical runs, displays missing
      targets, and links JSON/XHTML/raw evidence for each attempted run.
- [ ] AO2.7 and AO2.8 explicitly defer to this document for reporting and
      publication while retaining their own performance/safety gates.
- [ ] Report schema, path-safety, failed/incomplete-finalizer, matrix, and
      aggregate tests pass in CI; no physical M5/Windows execution is required
      to complete this documentation sprint.
- [ ] Quality-mgr reviews this plan before any implementation task is
      dispatched.

## References

- `templates/benchmark-report/benchmark-run.xhtml.j2`
- `templates/benchmark-report/benchmark-report.html.j2`
- `scripts/smoke/benchmark_report.py`
- `scripts/smoke/benchmark_schema.py`
- `.just/generate_report_index.py`
- `Justfile` (`benchmark`, `benchmark-report`, and `reports-index` recipes)
- `docs/plans/phase-ao2/sprint-AO2-7-m5-tcp-benchmark-parity.md`
- `docs/plans/phase-ao2/sprint-AO2-8-windows-tcp-benchmark-parity.md`
