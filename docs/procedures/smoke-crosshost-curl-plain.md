---
procedure: smoke-crosshost-curl-plain
family: smoke
runner: scripts/smoke/run_feature_smoke.py
evidence: site/reports/<run>/<procedure>.json
revisions:
  - rev: 1595424dca42905e6eb8ceb7b48d722cb8c8ddef
    date: 2026-09-12
    note: "current"
  - rev: c355f382a62b5cc94d47a321ec3fbbebcbe9f167
    date: 2026-07-26
    note: "historical runner revision"
---

## What this test proves
The `smoke-crosshost-curl-plain` procedure runs the named verification steps in order. Each step records an observable result in the run evidence. The page is addressed by the runner revision so a historical report remains explainable after the runner changes.

## Flow
```mermaid
flowchart LR
  setup[Prepare runner] --> steps[Execute procedure steps]
  steps --> evidence[Write immutable evidence JSON and HTML]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Run `doctor` | PASS or FAIL is recorded | `smoke-crosshost-curl-plain.json` cases |
| 2 | Run `advertised host` | PASS or FAIL is recorded | `smoke-crosshost-curl-plain.json` cases |
| 3 | Run `smoke case results` | PASS or FAIL is recorded | `smoke-crosshost-curl-plain.json` cases |


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
| 1 | Run `doctor` | PASS or FAIL is recorded | `smoke-crosshost-curl-plain.json` cases |
| 2 | Run `advertised host` | PASS or FAIL is recorded | `smoke-crosshost-curl-plain.json` cases |
| 3 | Run `smoke case results` | PASS or FAIL is recorded | `smoke-crosshost-curl-plain.json` cases |


## Revision c355f382 (2026-07-26)
historical runner revision.

```mermaid
flowchart LR
  setup[Prepare runner] --> steps[Execute procedure steps]
  steps --> evidence[Write immutable evidence JSON and HTML]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Run `doctor` | PASS or FAIL is recorded | `smoke-crosshost-curl-plain.json` cases |
| 2 | Run `advertised host` | PASS or FAIL is recorded | `smoke-crosshost-curl-plain.json` cases |
| 3 | Run `smoke case results` | PASS or FAIL is recorded | `smoke-crosshost-curl-plain.json` cases |

