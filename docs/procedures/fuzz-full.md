---
procedure: fuzz-full
family: fuzz
runner: .just/run_fuzz.py
evidence: site/reports/<run>/<procedure>.json
revisions:
  - rev: 1595424dca42905e6eb8ceb7b48d722cb8c8ddef
    date: 2026-09-12
    note: "current"
  - rev: 2f40a3a785cb9eedf50a439c5b85bf9571bac62e
    date: 2026-07-31
    note: "historical runner revision"
---

## What this test proves
The `fuzz-full` procedure runs the named verification steps in order. Each step records an observable result in the run evidence. The page is addressed by the runner revision so a historical report remains explainable after the runner changes.

## Flow
```mermaid
flowchart LR
  setup[Prepare runner] --> steps[Execute procedure steps]
  steps --> evidence[Write immutable evidence JSON and HTML]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Run `shape-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |
| 2 | Run `template-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |
| 3 | Run `boundary-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |
| 4 | Run `differential-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |


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
| 1 | Run `shape-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |
| 2 | Run `template-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |
| 3 | Run `boundary-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |
| 4 | Run `differential-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |


## Revision 2f40a3a7 (2026-07-31)
historical runner revision.

```mermaid
flowchart LR
  setup[Prepare runner] --> steps[Execute procedure steps]
  steps --> evidence[Write immutable evidence JSON and HTML]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Run `shape-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |
| 2 | Run `template-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |
| 3 | Run `boundary-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |
| 4 | Run `differential-probe` | PASS or FAIL is recorded | `fuzz-full.json` cases |

