---
title: Phase AF — 1.3.1 Reliability Recovery
status: complete
branch: integrate/phase-AF
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-AF
---

# Phase AF — 1.3.1 Reliability Recovery

## Decision

Use **Phase AF** for the 1.3.1 recovery. It is split into three production
sprints because the host-wide runtime invariant, observability/release-hardening
work, and the native send-input data path cannot credibly close at production
quality in one sprint. AF-1 is the release blocker; AF-2 and AF-3 may start
only after AF-1 has an accepted design and its process-level proof is green.

Every deliverable in the three sprint documents is a production-ready closure
commitment. A sprint cannot report success while one of its table rows, its
required documentation alignment, or its required validation is deferred.

AF-1, AF-2, and AF-3 are now merged on `integrate/phase-AF` at
`52c5c338`; `d5420b0f` is the docs-only follow-up that corrected the
accepted-line readiness record. PR #539 then merged that accepted line to
`develop` at `98a4e66c`, so this phase README is now complete as a dependency
artifact for later phases.

| Sprint | Closure | Sprint-local gate |
| --- | --- | --- |
| [AF-1: host singleton](af-1-host-singleton.md) | One `atm-daemon` per OS user/host, with no `ATM_HOME`, socket, or test exception bypass. | Required before any further full smoke that can launch a daemon, and before 1.3.1 RC. |
| [AF-2: observability and release gates](af-2-observability-release-gates.md) | Accurate doctor hook disclosure, actionable daemon errors, hermetic validation, and cutover safeguards. | Required before declaring 1.3.1 release-ready. |
| [AF-3: native send-input integrity](af-3-native-send-input-integrity.md) | `stdin`, inline, and file send sources reach the daemon with the intended bytes and typed local failures. | Required before declaring 1.3.1 release-ready. |

The authoritative closure records for AF-1, AF-2, and AF-3 now live on the
accepted `integrate/phase-AF` line. Historical feature branches remain part of
the execution record, but they are no longer the authoritative branch/worktree
for phase closure.

## Shared smoke-script integration contract

`scripts/smoke/run_thorough_shared_host.py` has one cross-sprint owner at a
time. AF1-D5 owns its base structure: the singleton preflight, process-count
capture, and cleanup assertion. AF-1 merges first. AF2-D4 rebases on that AF-1
base and adds installed-artifact selection and release-preflight assertions.
AF3-D3 then rebases on the merged AF-1 and AF-2 script and adds only the
inline/stdin/file durable-body matrix.

Every later change retains the preceding sprint's assertions: AF-2 must retain
AF-1 PID/count and cleanup assertions; AF-3 must retain both AF-1 and AF-2
assertions while adding its input matrix. A merge/rebase that removes or masks
an earlier assertion fails the later sprint's validation. The required merge
order is **AF-1 → AF-2 → AF-3**; parallel work may prepare patches, but its
final merge must rebase in that order.

## Governance and boundary coverage

| Required artifact | AF owner and required outcome |
| --- | --- |
| `docs/project-plan.md` | Phase registration and all-sprint closure summary are owned by this README. |
| `docs/requirements.md`, `docs/architecture.md`, `docs/adr/ADR-002-host-wide-daemon-singleton.md`, `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`, planned `docs/adr/ADR-026-host-singleton-and-durable-state-root.md`, `docs/adr/INDEX.md` | AF1-D0/D1 author ADR-026 to supersede ADR-002/005, align the one daemon/one durable-state-root contract, error codes, and ADR index without rewriting accepted history. |
| `docs/atm/{requirements,architecture,boundaries}.md`, `docs/atm-daemon-client/{requirements,architecture,boundaries}.md` | AF1-D1/D2 and AF3-D1 align CLI/client ownership and the no-daemon-stdin wire rule. |
| `docs/atm-core/{requirements,architecture,boundaries}.md`, `docs/atm-daemon/{requirements,architecture,boundaries}.md`, `docs/atm-daemon/protocol-icd.md`, planned `docs/adr/ADR-027-client-daemon-version-compatibility.md` | AF1-D3/D4, AF2-D1/D2/D5, and AF3-D1 align protocol, daemon admission, doctor, transport failure, and version-compatibility contracts. |
| `boundaries/atm-daemon-client/{daemon-bootstrap,rpc-envelope}.toml`, `boundaries/atm/local-socket-client-transport.toml`, `boundaries/atm-daemon/{host-ownership-daemon,socket-server-transport}.toml`, `boundaries/atm-core/{atm-protocol,config-doctor}.toml` | The sprint that changes a listed boundary updates its machine-readable contract and runs its named lint/review gate; no boundary change is docs-only. |
| `docs/testing-guidelines.md`, ADR-003, ADR-007, ADR-008, `scripts/lint_daemon_singleton.py` | AF1-D5/D6 own singleton/lifecycle and cross-platform test alignment; AF2-D4 and AF3-D3 extend the shared smoke only under the integration contract above. |
| `release-findings.json`, `reports/smoke/smoke-thorough.md`, `docs/plans/phase-af/readiness.md`, `docs/team-protocol.md` | AF2-D4 and AF3-D3 refresh issue disposition and release evidence; the readiness record is the phase-close gate, and team protocol remains the QA/triage routing rule. |

## Phase release decision criteria

This is the single authoritative Phase AF release checklist.

1. AF-1's process-level singleton suite is green on macOS, Linux, and Windows.
2. AF2-D1 through AF2-D5 validations are green using the release artifacts,
   not `cargo run` or an arbitrary PATH binary.
3. AF-3's release-binary inline/stdin/file input matrix is green against a
   daemon with null stdin.
4. A fresh user-state database can create the team and roster through native
   1.3.1 commands, send/read/ack a message, and show a healthy doctor with no
   unexpected retained errors.
5. The release report lists exact binary versions, PID/count evidence, hook
   selection, doctor status, and any non-empty error snapshot. Any unexpected
   error record is a release blocker until classified and waived explicitly.

## Evidence baseline

The authoritative smoke evidence is maintained in
`reports/smoke/smoke-thorough.md` and `release-findings.json`; its initial
Phase AF capture was committed as `9e01e19e` and later plan corrections are
recorded on this branch:

- `SMOKE-FIND-001` is release-blocking: three daemons ran concurrently when
  launchers used distinct `ATM_HOME` roots.
- `SMOKE-FIND-005` and `SMOKE-FIND-007` are the two user-visible diagnostics
  gaps: healthy doctor output hides retained daemon errors and active nudge
  overrides.
- `SMOKE-FIND-002`, `003`, `004`, and `006` supply the release-process and
  configuration follow-through for AF-2.
- The post-plan native-CLI smoke finding (`atm send <to> --stdin`) is AF-3:
  its bytes are currently sent to the daemon as a `Stdin` marker even though
  the daemon process is intentionally spawned with null stdin.

No Phase AF sprint permits a test-only alternate runtime root, endpoint, lock,
or daemon launch path. Tests must prove the production invariant from an
isolated OS user/CI host, not weaken it in the product.
