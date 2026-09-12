---
procedure: smoke-admission-capacity
family: smoke
runner: scripts/smoke/run_feature_smoke.py
evidence: site/reports/<run>/smoke-admission-capacity.json
revisions:
  - rev: 8ee8a3910057255b13a7ef153e01d54f90b99824
    date: 2026-09-07
    note: "current admission boundary runner"
  - rev: b193b950fad78b1780b54bc8e4cab6cc9582a975
    date: 2026-09-06
    note: "check doctor after roster setup"
  - rev: 0817d87bc2e43480fa3763bf12ce89388da4d599
    date: 2026-09-04
    note: "probe the launched daemon runtime"
  - rev: d3a601ab039a58b4cc0b31dde4d915554d609bfc
    date: 2026-09-04
    note: "rebuild sqlite writer probe per run"
  - rev: 0656b8a28d6cf0128c6349379c0def1015f8a5d0
    date: 2026-09-03
    note: "cover invariant lane publication"
---

## What this test proves
The admission-capacity lane drives the local public admission boundary at its bounded target rate and records throughput, rejection, and durability evidence. The result is a benchmark-shaped smoke artifact with a stable procedure page.

## Flow
```mermaid
flowchart LR
  setup[Setup smoke-admission-capacity]
  case1[group1: clean daemon preflight]
  case2[group2: durability after restart]
  teardown[Validate and close]
  evidence[Write immutable evidence]
  setup --> case1
  case1 --> case2
  case2 --> teardown
  teardown --> evidence
  note[runner revision 8ee8a391]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `clean daemon preflight` | The named check reports PASS or FAIL at revision `8ee8a391` | `smoke-admission-capacity.json` cases |
| 2 | Execute `bounded admission writes` | The named check reports PASS or FAIL at revision `8ee8a391` | `smoke-admission-capacity.json` cases |
| 3 | Execute `durability after restart` | The named check reports PASS or FAIL at revision `8ee8a391` | `smoke-admission-capacity.json` cases |
| 4 | Execute `throughput verdict` | The named check reports PASS or FAIL at revision `8ee8a391` | `smoke-admission-capacity.json` cases |

## Evidence layout
The admission runner leaves its bounded campaign JSON and rendered report under `site/reports/`. The payload records the preflight, write, durability, and throughput observations; the envelope makes the run discoverable in the public index.

## Changes
The revision sections below preserve the admission runner history. Each section names the implementation change that established its procedure order.

## Revision b193b950 (2026-09-06)
The runner checks the daemon doctor result after roster setup before measuring the admission lanes.

```mermaid
flowchart LR
  setup[Prepare disposable roster] --> checks[Run admission and durability cases]
  checks --> teardown[Close daemon and verify artifacts]
  teardown --> evidence[Write campaign evidence]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `clean daemon preflight` | Doctor is ready after roster setup | `smoke-admission-capacity.json` cases |
| 2 | Execute `bounded admission writes` | The bounded write rate is recorded | `smoke-admission-capacity.json` cases |
| 3 | Execute `durability after restart` | Restart preserves the durable rows | `smoke-admission-capacity.json` cases |
| 4 | Execute `throughput verdict` | The reviewed threshold verdict is recorded | `smoke-admission-capacity.json` cases |

## Revision 0817d87b (2026-09-04)
The runner probes the launched daemon runtime before entering the bounded admission measurement.

```mermaid
flowchart LR
  setup[Launch and probe runtime] --> checks[Run admission and durability cases]
  checks --> teardown[Close daemon and verify artifacts]
  teardown --> evidence[Write campaign evidence]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `clean daemon preflight` | The launched runtime answers doctor | `smoke-admission-capacity.json` cases |
| 2 | Execute `bounded admission writes` | The bounded write rate is recorded | `smoke-admission-capacity.json` cases |
| 3 | Execute `durability after restart` | Restart preserves the durable rows | `smoke-admission-capacity.json` cases |
| 4 | Execute `throughput verdict` | The reviewed threshold verdict is recorded | `smoke-admission-capacity.json` cases |

## Revision d3a601ab (2026-09-04)
The runner rebuilds its SQLite writer probe for each invocation, keeping the capacity evidence isolated.

```mermaid
flowchart LR
  setup[Create per-run SQLite probe] --> checks[Run admission and durability cases]
  checks --> teardown[Close daemon and verify artifacts]
  teardown --> evidence[Write campaign evidence]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `clean daemon preflight` | The isolated probe starts cleanly | `smoke-admission-capacity.json` cases |
| 2 | Execute `bounded admission writes` | The bounded write rate is recorded | `smoke-admission-capacity.json` cases |
| 3 | Execute `durability after restart` | Restart preserves the durable rows | `smoke-admission-capacity.json` cases |
| 4 | Execute `throughput verdict` | The reviewed threshold verdict is recorded | `smoke-admission-capacity.json` cases |

## Revision 0656b8a2 (2026-09-03)
The runner covers invariant-lane publication while retaining the same bounded capacity and durability observations.

```mermaid
flowchart LR
  setup[Prepare invariant lane] --> checks[Run admission and durability cases]
  checks --> teardown[Close daemon and verify artifacts]
  teardown --> evidence[Write campaign evidence]
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `clean daemon preflight` | The invariant lane starts from a clean daemon | `smoke-admission-capacity.json` cases |
| 2 | Execute `bounded admission writes` | The bounded write rate is recorded | `smoke-admission-capacity.json` cases |
| 3 | Execute `durability after restart` | Restart preserves the durable rows | `smoke-admission-capacity.json` cases |
| 4 | Execute `throughput verdict` | The reviewed threshold verdict is recorded | `smoke-admission-capacity.json` cases |

## Evidence layout
The runner writes immutable evidence for `smoke-admission-capacity` beneath `site/reports/`. The JSON payload is the source for case or worker outcomes; the rendered report and envelope provide the public navigation entry.

## Changes
The revision sections below are the retained runner-history backfill. Each section records the source revision and its own ordered procedure description.

## Revision 8ee8a391 (2026-09-07)
The runner change `fix: scope daemon singleton per account` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-admission-capacity]
  case1[group1: clean daemon preflight]
  case2[group2: durability after restart]
  teardown[Validate and close]
  evidence[Write immutable evidence]
  setup --> case1
  case1 --> case2
  case2 --> teardown
  teardown --> evidence
  note[runner revision 8ee8a391]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `clean daemon preflight` | The named check reports PASS or FAIL at revision `8ee8a391` | `smoke-admission-capacity.json` cases |
| 2 | Execute `bounded admission writes` | The named check reports PASS or FAIL at revision `8ee8a391` | `smoke-admission-capacity.json` cases |
| 3 | Execute `durability after restart` | The named check reports PASS or FAIL at revision `8ee8a391` | `smoke-admission-capacity.json` cases |
| 4 | Execute `throughput verdict` | The named check reports PASS or FAIL at revision `8ee8a391` | `smoke-admission-capacity.json` cases |
