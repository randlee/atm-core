---
title: Phase AF Readiness
status: in_progress
branch: integrate/phase-AF
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-AF
---

# Phase AF Readiness

This is the authoritative closeout gate for the Phase AF plan. It records the
required execution evidence; it does not claim that the 1.3.1 implementation
has shipped.

AF-1, AF-2, and AF-3 are all merged into `integrate/phase-AF` (commit
`52c5c338`). PR #539 (`integrate/phase-AF` -> `develop`) is the accepted
develop candidate under phase-end review. Phase readiness remains
`in_progress` until quality-mgr's phase-end QA gate and arch-ctm's
production-readiness review both close, and the user authorizes merging
PR #539.

| Sprint | Plan closure | Required accepted-line evidence |
| --- | --- | --- |
| AF-1 | Host singleton design complete | Cross-`ATM_HOME` process proof on macOS, Linux, and Windows shows one daemon, endpoint, and durable-state root; lifecycle cleanup leaves no owned artifacts. |
| AF-2 | Observability and release-gate design complete | Installed-artifact smoke records PID/count, hook selection, doctor liveness/readiness, classified errors, capacity/deadline behavior, and version-cutover no-write proof. |
| AF-3 | Native send-input design complete | Release-binary inline/stdin/file matrix compares durable readback bytes and retains AF-1/AF-2 shared-smoke assertions. |

## Exit Gate

Phase AF is ready for a 1.3.1 release decision only when every AF-1, AF-2,
and AF-3 validation named in [`README.md`](./README.md) is green on the
accepted `develop` candidate. Any second daemon, leaked lifecycle artifact,
unexpected retained error, incompatible-pair write, or input byte mismatch is
a release blocker until corrected and revalidated.
