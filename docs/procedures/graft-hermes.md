---
procedure: graft-hermes
family: smoke
runner: scripts/phase-ai/run_hermes_graft_live.py
evidence: site/reports/<run>/<procedure>.json
revisions:
  - rev: 1595424dca42905e6eb8ceb7b48d722cb8c8ddef
    date: 2026-09-12
    note: "current"
  - rev: bab416ca946b8776ea6c5514c79868a447a6683e
    date: 2026-08-10
    note: "historical runner revision"
---

## What this test proves
The `graft-hermes` procedure runs the named verification steps in order. Each step records an observable result in the run evidence. The page is addressed by the runner revision so a historical report remains explainable after the runner changes.

## Flow
```mermaid
flowchart LR
  setup[Prepare runner] --> steps[Execute procedure steps]
  steps --> evidence[Write immutable evidence JSON and HTML]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Run `doctor` | PASS or FAIL is recorded | `graft-hermes.json` cases |
| 2 | Run `graft outbound durable write and receiver round trip` | PASS or FAIL is recorded | `graft-hermes.json` cases |


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
| 1 | Run `doctor` | PASS or FAIL is recorded | `graft-hermes.json` cases |
| 2 | Run `graft outbound durable write and receiver round trip` | PASS or FAIL is recorded | `graft-hermes.json` cases |


## Revision bab416ca (2026-08-10)
historical runner revision.

```mermaid
flowchart LR
  setup[Prepare runner] --> steps[Execute procedure steps]
  steps --> evidence[Write immutable evidence JSON and HTML]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Run `doctor` | PASS or FAIL is recorded | `graft-hermes.json` cases |
| 2 | Run `graft outbound durable write and receiver round trip` | PASS or FAIL is recorded | `graft-hermes.json` cases |

