---
procedure: read-query-benchmark
family: benchmark
runner: scripts/smoke/benchmark_report.py
evidence: site/reports/<run>/<procedure>.json
revisions:
  - rev: 1595424dca42905e6eb8ceb7b48d722cb8c8ddef
    date: 2026-09-12
    note: "current"
  - rev: bedcb4b85af06a00ed527adff0e2817d2fca7b90
    date: 2026-07-31
    note: "historical runner revision"
---

## What this test proves
The `read-query-benchmark` procedure runs the named verification steps in order. Each step records an observable result in the run evidence. The page is addressed by the runner revision so a historical report remains explainable after the runner changes.

## Flow
```mermaid
flowchart LR
  setup[Prepare runner] --> steps[Execute procedure steps]
  steps --> evidence[Write immutable evidence JSON and HTML]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Run `sqlite` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |
| 2 | Run `uds` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |
| 3 | Run `tcp` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |
| 4 | Run `mTLS` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |


## Evidence layout
The runner writes a procedure JSON payload beside its rendered report and an envelope used by the public report index. Existing evidence artifacts are immutable.

## Changes
This page is backfilled from the runner history; revision-specific notes are listed below.

## Revision 1595424d (2026-09-12)
current.

```mermaid
flowchart LR
  setup[Prepare runner] --> steps[Execute procedure steps]
  steps --> evidence[Write immutable evidence JSON and HTML]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Run `sqlite` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |
| 2 | Run `uds` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |
| 3 | Run `tcp` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |
| 4 | Run `mTLS` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |


## Revision bedcb4b8 (2026-07-31)
historical runner revision.

```mermaid
flowchart LR
  setup[Prepare runner] --> steps[Execute procedure steps]
  steps --> evidence[Write immutable evidence JSON and HTML]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Run `sqlite` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |
| 2 | Run `uds` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |
| 3 | Run `tcp` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |
| 4 | Run `mTLS` | PASS or FAIL is recorded | `read-query-benchmark.json` cases |

