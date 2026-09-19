---
procedure: smoke-peer-preflight
family: smoke
runner: scripts/smoke/run_feature_smoke.py
evidence: site/reports/<run>/smoke-peer-preflight.json
revisions:
  - rev: 7475fed9e2a341232f71afba61d8c21685789610
    date: 2026-09-04
    note: "current runner revision"
  - rev: 934586917b61bb521635c5678fe87b5acb3f612c
    date: 2026-08-20
    note: "test: prove mTLS rejects unauthenticated clients"
  - rev: 513b33745edda8e38e694f6d2867d3b4de8fb2f0
    date: 2026-08-07
    note: "fix(smoke): index every hardware smoke run"
  - rev: 8043ce649c92cb78cff038a003acc76c599f4dc7
    date: 2026-08-07
    note: "fix(al9): discover same-host smoke address at runtime"
  - rev: c355f382a62b5cc94d47a321ec3fbbebcbe9f167
    date: 2026-07-26
    note: "feat(smoke): add progressive live daemon features"
---

## What this test proves
This preflight lane verifies the local endpoint and each remote peer's doctor response and advertised host before any cross-host message work. A failed peer is recorded as a bounded preflight result.

## Flow
```mermaid
flowchart LR
  setup[Setup smoke-peer-preflight]
  case1[group1: doctor]
  case2[group2: local-IP acknowledgement reply]
  case3[group3: rand-m4.local doctor/version]
  teardown[Validate and close]
  evidence[Write immutable evidence]
  setup --> case1
  case1 --> case2
  case2 --> case3
  case3 --> teardown
  teardown --> evidence
  note[runner revision 7475fed9]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `doctor` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 2 | Execute `advertised host` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 3 | Execute `local-IP send/read/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 4 | Execute `local-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 5 | Execute `local-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 6 | Execute `loopback-IP send/read/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 7 | Execute `loopback-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 8 | Execute `loopback-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 9 | Execute `rand-m4.local doctor/version` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 10 | Execute `rand-m4.local advertised host` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |

## Evidence layout
The runner writes immutable evidence for `smoke-peer-preflight` beneath `site/reports/`. The JSON payload is the source for case or worker outcomes; the rendered report and envelope provide the public navigation entry.

## Changes
The revision sections below are the retained runner-history backfill. Each section records the source revision and its own ordered procedure description.

## Revision 7475fed9 (2026-09-04)
The runner change `fix(smoke): configure direct-peer port` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-peer-preflight]
  case1[group1: doctor]
  case2[group2: local-IP acknowledgement reply]
  case3[group3: rand-m4.local doctor/version]
  teardown[Validate and close]
  evidence[Write immutable evidence]
  setup --> case1
  case1 --> case2
  case2 --> case3
  case3 --> teardown
  teardown --> evidence
  note[runner revision 7475fed9]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `doctor` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 2 | Execute `advertised host` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 3 | Execute `local-IP send/read/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 4 | Execute `local-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 5 | Execute `local-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 6 | Execute `loopback-IP send/read/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 7 | Execute `loopback-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 8 | Execute `loopback-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 9 | Execute `rand-m4.local doctor/version` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |
| 10 | Execute `rand-m4.local advertised host` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-peer-preflight.json` cases |

## Revision 93458691 (2026-08-20)
The runner change `test: prove mTLS rejects unauthenticated clients` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-peer-preflight]
  case1[group1: doctor]
  case2[group2: local-IP acknowledgement reply]
  case3[group3: rand-m4.local doctor/version]
  teardown[Validate and close]
  evidence[Write immutable evidence]
  setup --> case1
  case1 --> case2
  case2 --> case3
  case3 --> teardown
  teardown --> evidence
  note[runner revision 93458691]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `doctor` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |
| 2 | Execute `advertised host` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |
| 3 | Execute `local-IP send/read/content` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |
| 4 | Execute `local-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |
| 5 | Execute `local-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |
| 6 | Execute `loopback-IP send/read/content` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |
| 7 | Execute `loopback-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |
| 8 | Execute `loopback-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |
| 9 | Execute `rand-m4.local doctor/version` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |
| 10 | Execute `rand-m4.local advertised host` | The named check reports PASS or FAIL at revision `93458691` | `smoke-peer-preflight.json` cases |

## Revision 513b3374 (2026-08-07)
The runner change `fix(smoke): index every hardware smoke run` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-peer-preflight]
  case1[group1: doctor]
  case2[group2: local-IP acknowledgement reply]
  case3[group3: rand-m4.local doctor/version]
  teardown[Validate and close]
  evidence[Write immutable evidence]
  setup --> case1
  case1 --> case2
  case2 --> case3
  case3 --> teardown
  teardown --> evidence
  note[runner revision 513b3374]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `doctor` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |
| 2 | Execute `advertised host` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |
| 3 | Execute `local-IP send/read/content` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |
| 4 | Execute `local-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |
| 5 | Execute `local-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |
| 6 | Execute `loopback-IP send/read/content` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |
| 7 | Execute `loopback-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |
| 8 | Execute `loopback-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |
| 9 | Execute `rand-m4.local doctor/version` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |
| 10 | Execute `rand-m4.local advertised host` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-peer-preflight.json` cases |

## Revision 8043ce64 (2026-08-07)
The runner change `fix(al9): discover same-host smoke address at runtime` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-peer-preflight]
  case1[group1: doctor]
  case2[group2: local-IP acknowledgement reply]
  case3[group3: rand-m4.local doctor/version]
  teardown[Validate and close]
  evidence[Write immutable evidence]
  setup --> case1
  case1 --> case2
  case2 --> case3
  case3 --> teardown
  teardown --> evidence
  note[runner revision 8043ce64]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `doctor` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |
| 2 | Execute `advertised host` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |
| 3 | Execute `local-IP send/read/content` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |
| 4 | Execute `local-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |
| 5 | Execute `local-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |
| 6 | Execute `loopback-IP send/read/content` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |
| 7 | Execute `loopback-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |
| 8 | Execute `loopback-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |
| 9 | Execute `rand-m4.local doctor/version` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |
| 10 | Execute `rand-m4.local advertised host` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-peer-preflight.json` cases |

## Revision c355f382 (2026-07-26)
The runner change `feat(smoke): add progressive live daemon features` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-peer-preflight]
  case1[group1: doctor]
  case2[group2: local-IP acknowledgement reply]
  case3[group3: rand-m4.local doctor/version]
  teardown[Validate and close]
  evidence[Write immutable evidence]
  setup --> case1
  case1 --> case2
  case2 --> case3
  case3 --> teardown
  teardown --> evidence
  note[runner revision c355f382]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Execute `doctor` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
| 2 | Execute `advertised host` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
| 3 | Execute `local-IP send/read/content` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
| 4 | Execute `local-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
| 5 | Execute `local-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
| 6 | Execute `loopback-IP send/read/content` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
| 7 | Execute `loopback-IP requires-ack delivery/content` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
| 8 | Execute `loopback-IP acknowledgement reply delivery/content` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
| 9 | Execute `rand-m4.local doctor/version` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
| 10 | Execute `rand-m4.local advertised host` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-peer-preflight.json` cases |
