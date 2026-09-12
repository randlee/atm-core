---
procedure: smoke-normal
family: smoke
runner: scripts/smoke/run_feature_smoke.py
evidence: site/reports/<run>/smoke-normal.json
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
The normal fixture extends the fast command checks with acknowledgement, list, clear, team administration, roster repair, and durable-read behavior. Reviewers can compare every row with the command and test name in the fixture source.

## Flow
```mermaid
flowchart LR
  setup[Setup smoke-normal]
  case1[group1: AD11-CMD-ACK-001]
  case2[group2: AD11-CMD-TEAMS-BACKUP-001]
  case3[group3: AD17-CI-001]
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
| 1 | Execute `AD11-CMD-ACK-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 2 | Execute `AD11-CMD-LIST-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 3 | Execute `AD11-CMD-CLEAR-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 4 | Execute `AD11-CMD-TEAMS-ADD-MEMBER-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 5 | Execute `AD11-CMD-TEAMS-UPDATE-MEMBER-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 6 | Execute `AD11-CMD-TEAMS-BACKUP-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 7 | Execute `AD11-CMD-TEAMS-RESTORE-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 8 | Execute `AD11-ROSTER-REPAIR-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 9 | Execute `AD17-ULID-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 10 | Execute `AD17-READ-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 11 | Execute `AD17-CI-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 12 | Execute `AD18-RUNTIME-ROOT-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 13 | Execute `AD18-RUNTIME-ROOT-002` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 14 | Execute `AD19-READ-OUTPUT-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 15 | Execute `AD20-READ-CONTAINS-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |

## Evidence layout
The runner writes immutable evidence for `smoke-normal` beneath `site/reports/`. The JSON payload is the source for case or worker outcomes; the rendered report and envelope provide the public navigation entry.

## Changes
The revision sections below are the retained runner-history backfill. Each section records the source revision and its own ordered procedure description.

## Revision 7475fed9 (2026-09-04)
The runner change `fix(smoke): configure direct-peer port` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-normal]
  case1[group1: AD11-CMD-ACK-001]
  case2[group2: AD11-CMD-TEAMS-BACKUP-001]
  case3[group3: AD17-CI-001]
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
| 1 | Execute `AD11-CMD-ACK-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 2 | Execute `AD11-CMD-LIST-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 3 | Execute `AD11-CMD-CLEAR-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 4 | Execute `AD11-CMD-TEAMS-ADD-MEMBER-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 5 | Execute `AD11-CMD-TEAMS-UPDATE-MEMBER-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 6 | Execute `AD11-CMD-TEAMS-BACKUP-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 7 | Execute `AD11-CMD-TEAMS-RESTORE-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 8 | Execute `AD11-ROSTER-REPAIR-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 9 | Execute `AD17-ULID-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 10 | Execute `AD17-READ-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 11 | Execute `AD17-CI-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 12 | Execute `AD18-RUNTIME-ROOT-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 13 | Execute `AD18-RUNTIME-ROOT-002` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 14 | Execute `AD19-READ-OUTPUT-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |
| 15 | Execute `AD20-READ-CONTAINS-001` | The named check reports PASS or FAIL at revision `7475fed9` | `smoke-normal.json` cases |

## Revision 93458691 (2026-08-20)
The runner change `test: prove mTLS rejects unauthenticated clients` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-normal]
  case1[group1: AD11-CMD-ACK-001]
  case2[group2: AD11-CMD-TEAMS-BACKUP-001]
  case3[group3: AD17-CI-001]
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
| 1 | Execute `AD11-CMD-ACK-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 2 | Execute `AD11-CMD-LIST-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 3 | Execute `AD11-CMD-CLEAR-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 4 | Execute `AD11-CMD-TEAMS-ADD-MEMBER-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 5 | Execute `AD11-CMD-TEAMS-UPDATE-MEMBER-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 6 | Execute `AD11-CMD-TEAMS-BACKUP-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 7 | Execute `AD11-CMD-TEAMS-RESTORE-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 8 | Execute `AD11-ROSTER-REPAIR-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 9 | Execute `AD17-ULID-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 10 | Execute `AD17-READ-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 11 | Execute `AD17-CI-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 12 | Execute `AD18-RUNTIME-ROOT-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 13 | Execute `AD18-RUNTIME-ROOT-002` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 14 | Execute `AD19-READ-OUTPUT-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |
| 15 | Execute `AD20-READ-CONTAINS-001` | The named check reports PASS or FAIL at revision `93458691` | `smoke-normal.json` cases |

## Revision 513b3374 (2026-08-07)
The runner change `fix(smoke): index every hardware smoke run` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-normal]
  case1[group1: AD11-CMD-ACK-001]
  case2[group2: AD11-CMD-TEAMS-BACKUP-001]
  case3[group3: AD17-CI-001]
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
| 1 | Execute `AD11-CMD-ACK-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 2 | Execute `AD11-CMD-LIST-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 3 | Execute `AD11-CMD-CLEAR-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 4 | Execute `AD11-CMD-TEAMS-ADD-MEMBER-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 5 | Execute `AD11-CMD-TEAMS-UPDATE-MEMBER-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 6 | Execute `AD11-CMD-TEAMS-BACKUP-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 7 | Execute `AD11-CMD-TEAMS-RESTORE-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 8 | Execute `AD11-ROSTER-REPAIR-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 9 | Execute `AD17-ULID-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 10 | Execute `AD17-READ-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 11 | Execute `AD17-CI-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 12 | Execute `AD18-RUNTIME-ROOT-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 13 | Execute `AD18-RUNTIME-ROOT-002` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 14 | Execute `AD19-READ-OUTPUT-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |
| 15 | Execute `AD20-READ-CONTAINS-001` | The named check reports PASS or FAIL at revision `513b3374` | `smoke-normal.json` cases |

## Revision 8043ce64 (2026-08-07)
The runner change `fix(al9): discover same-host smoke address at runtime` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-normal]
  case1[group1: AD11-CMD-ACK-001]
  case2[group2: AD11-CMD-TEAMS-BACKUP-001]
  case3[group3: AD17-CI-001]
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
| 1 | Execute `AD11-CMD-ACK-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 2 | Execute `AD11-CMD-LIST-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 3 | Execute `AD11-CMD-CLEAR-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 4 | Execute `AD11-CMD-TEAMS-ADD-MEMBER-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 5 | Execute `AD11-CMD-TEAMS-UPDATE-MEMBER-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 6 | Execute `AD11-CMD-TEAMS-BACKUP-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 7 | Execute `AD11-CMD-TEAMS-RESTORE-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 8 | Execute `AD11-ROSTER-REPAIR-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 9 | Execute `AD17-ULID-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 10 | Execute `AD17-READ-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 11 | Execute `AD17-CI-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 12 | Execute `AD18-RUNTIME-ROOT-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 13 | Execute `AD18-RUNTIME-ROOT-002` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 14 | Execute `AD19-READ-OUTPUT-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |
| 15 | Execute `AD20-READ-CONTAINS-001` | The named check reports PASS or FAIL at revision `8043ce64` | `smoke-normal.json` cases |

## Revision c355f382 (2026-07-26)
The runner change `feat(smoke): add progressive live daemon features` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Setup smoke-normal]
  case1[group1: AD11-CMD-ACK-001]
  case2[group2: AD11-CMD-TEAMS-BACKUP-001]
  case3[group3: AD17-CI-001]
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
| 1 | Execute `AD11-CMD-ACK-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 2 | Execute `AD11-CMD-LIST-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 3 | Execute `AD11-CMD-CLEAR-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 4 | Execute `AD11-CMD-TEAMS-ADD-MEMBER-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 5 | Execute `AD11-CMD-TEAMS-UPDATE-MEMBER-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 6 | Execute `AD11-CMD-TEAMS-BACKUP-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 7 | Execute `AD11-CMD-TEAMS-RESTORE-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 8 | Execute `AD11-ROSTER-REPAIR-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 9 | Execute `AD17-ULID-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 10 | Execute `AD17-READ-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 11 | Execute `AD17-CI-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 12 | Execute `AD18-RUNTIME-ROOT-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 13 | Execute `AD18-RUNTIME-ROOT-002` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 14 | Execute `AD19-READ-OUTPUT-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
| 15 | Execute `AD20-READ-CONTAINS-001` | The named check reports PASS or FAIL at revision `c355f382` | `smoke-normal.json` cases |
