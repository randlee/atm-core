---
title: Phase AF Readiness
status: complete
branch: integrate/phase-AF
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-AF
---

# Phase AF Readiness

This is the authoritative closeout gate for the Phase AF plan. It records the
required execution evidence; it does not claim that the 1.3.1 implementation
has shipped.

AF-1, AF-2, and AF-3 are all merged into `integrate/phase-AF` (commit
`52c5c338`). PR #539 then merged that accepted line to `develop` at
`98a4e66c`. Phase AF therefore remains the accepted closed same-host
dependency line for later phases.

| Sprint | Plan closure | Required accepted-line evidence |
| --- | --- | --- |
| AF-1 | Host singleton design complete | Cross-`ATM_HOME` process proof on macOS, Linux, and Windows shows one daemon, endpoint, and durable-state root; lifecycle cleanup leaves no owned artifacts. |
| AF-2 | Observability and release-gate design complete | Installed-artifact smoke records PID/count, hook selection, doctor liveness/readiness, classified errors, capacity/deadline behavior, and version-cutover no-write proof. |
| AF-3 | Native send-input design complete | Release-binary inline/stdin/file matrix compares durable readback bytes and retains AF-1/AF-2 shared-smoke assertions. |

## Accepted-line evidence ledger

| Deliverable | Status | Accepted-line evidence |
| --- | --- | --- |
| `AF1-D5` | closed | Merged by PR #535 at `dd61622e`. Deliverable commits `e5051fba`, `164f2b32`, `30b31ab8`, and `a071887b` closed the host-scoped singleton gate, isolated the shared-host smoke from ambient daemons, and retained the mandatory cleanup assertion. PR #535 finished 8/8 CI-green (`Format check`, `Clippy`, `Just lint` x3, `Test` x3). |
| `AF2-D4` | closed | Merged by PR #537 at `2cfe358c`. Deliverable commits `14288caa` and `15f1a3ac` closed installed-artifact smoke selection and the shared-host release-preflight row; the accepted line retains AF-1 PID/count and leak assertions while AF-3 builds on top. PR #537 finished 8/8 CI-green. |
| `AF2-D5` | closed | Merged by PR #537 at `2cfe358c`. Deliverable commits `58a0a234`, `e7e7da1d`, `f0511687`, `7c15b490`, and `0b208fda` closed the compatibility typestate gate, ADR-027/documentation alignment, no-write cutover behavior, and the non-colliding classified-failure sentinel. PR #537 finished 8/8 CI-green. |
| `AF3-D3` | closed | Merged by PR #538 at `52c5c338`. Deliverable commits `be2a1b79` and `068613ee` closed release-binary inline/stdin/file durable readback and refreshed the accepted-line smoke evidence SHA. PR #538 finished 8/8 CI-green. |

## Exit Gate

Phase AF is ready for a 1.3.1 release decision only when every AF-1, AF-2,
and AF-3 validation named in [`README.md`](./README.md) is green on the
accepted `develop` candidate. Any second daemon, leaked lifecycle artifact,
unexpected retained error, incompatible-pair write, or input byte mismatch is
a release blocker until corrected and revalidated.
