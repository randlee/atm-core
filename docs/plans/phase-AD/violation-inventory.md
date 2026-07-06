---
title: Phase AD Graft Boundary Violation Inventory
status: active
branch: plan/daemon-graft-boundary-reset
worktree: /Users/randlee/Documents/github/atm-core-worktrees/plan/daemon-graft-boundary-reset
---

# Phase AD Graft Boundary Violation Inventory

## Purpose

This document records the graft/session architectural drift that still blocks
`Phase AD` release readiness after the original `AD.1` through `AD.11` line.

It is not a speculative cleanup list. Each item below identifies concrete code
or doc surface that currently treats graft-specific session, queue, or stream
behavior as shared daemon/core infrastructure.

## Accepted Boundary Restatement

The intended boundary is:

- `atm send` persists a durable message
- after persistence, ATM optionally emits a post-send event through one
  capability seam
- tmux is one receiver implementation of that seam
- `atm-graft` is one receiver implementation of that seam
- ATM owns emission, logging, and sender-visible warnings when emission fails
- ATM does not own receiver-side consumption after successful emission
- receiver-specific active/inactive state may exist, but it must stay behind
  the receiver-owned implementation boundary
- daemon request routing, shared protocol DTOs, and transport receive loops
  must not model graft-specific session registration, fetch/drain, queue, or
  stream control as shared ATM infrastructure

## Code Violations

| Area | Current leak | Required correction |
|---|---|---|
| `crates/atm-core/src/boundary/mod.rs` | `RequestDispatcher` exposes `dispatch_advisory_stream(...)`, and the shared boundary module defines `AdvisoryStreamSink` for one receiver implementation. | Remove graft-specific stream dispatch from the shared dispatcher boundary. The accepted dispatcher surface returns to unary request routing only. |
| `crates/atm-core/src/graft.rs` | `AdvisorySessionPort`, `AdvisorySessionId`, `AdvisorySessionState`, `AdvisorySessionRegistrationRequest`, `AdvisoryFetchRequest`, `AdvisoryDrainRequest`, and `AdvisoryStreamRequest` model daemon-owned graft session lifecycle as shared `atm-core` infrastructure. | Delete the shared advisory session protocol surface from `atm-core`. Keep only the thin graft client contract actually required by retained ATM semantics. |
| `crates/atm-daemon-client/src/wire.rs` | `MessageKind` reserves first-class daemon packet families for `AdvisoryRegister`, `AdvisoryUnregister`, `AdvisoryFetch`, `AdvisoryDrain`, and `AdvisoryStream`. | Remove graft-only advisory packet kinds from the accepted daemon wire registry. |
| `crates/atm-daemon/src/runtime_health.rs` | `DaemonRequestDispatcher` routes `RequestEnvelope::AdvisoryRegister`, `AdvisoryUnregister`, `AdvisoryFetch`, `AdvisoryDrain`, and implements `dispatch_advisory_stream(...)`. | Remove graft-specific routing from the daemon dispatcher. The dispatcher must not own one receiver implementation's session protocol. |
| `crates/atm-daemon/src/advisory_runtime.rs` | `AdvisoryRuntime` owns graft session registration, per-session nudge queues, dropped-count bookkeeping, and stream loop behavior. | Delete daemon-owned graft session runtime state and direct callers. |
| `crates/atm-daemon/src/local_ipc_transport/request_worker.rs` | The local IPC worker special-cases `RequestEnvelope::AdvisoryStream` and owns a dedicated stream sink path. | Remove receiver-specific streaming logic from the transport worker so the receive loop returns to thin framed unary dispatch. |
| `crates/atm-daemon/src/tests_advisory.rs` and advisory-specific test seams | Daemon tests currently normalize daemon-owned graft session registration, fetch/drain, and stream behavior as core runtime obligations. | Remove the advisory-runtime test lane and replace it with tests that cover the accepted post-send seam only. |
| `crates/atm-graft/src/lib.rs`, `runtime.rs`, `transport.rs` | `atm-graft` is coupled to daemon-owned advisory registration, fetch/drain, and dedicated advisory-stream transport. | Reset `atm-graft` to a thin receiver implementation that owns any remaining receiver-side state internally and no longer depends on shared advisory session protocol families. |

## Architecture And Requirements Drift

These docs currently bless the same leak and therefore must be corrected as
part of the boundary-reset line:

| Document | Current drift |
|---|---|
| `docs/atm-daemon/requirements.md` | Declares `advisory register`, `advisory unregister`, `advisory fetch`, `advisory drain`, and `advisory stream` as daemon packet families and states that one live advisory stream per active embedded client session is a production requirement. |
| `docs/atm-daemon/protocol-icd.md` | Documents advisory register/unregister/fetch/drain/stream as first-class public packet kinds and envelope mappings. |
| `docs/atm-graft/architecture.md` | Requires a dedicated daemon advisory-stream connection, daemon-owned bounded pending-nudge state, and explicit graft session lifecycle states owned around that daemon session model. |
| `docs/atm-graft/requirements.md` | Carries the same daemon-owned persistent receive-loop and bounded queue assumptions into the published graft requirements surface. |
| `docs/plans/phase-AD/sprint-AD8.md` | Still frames the accepted graft path as a daemon/graft advisory-session seam rather than a thin post-send receiver implementation. |

## Review Request

Quality review must classify each item in one of two ways:

- accepted architectural violation to remove
- accepted architecture/doc requirement that explicitly authorizes the current
  scope, with the exact supporting ADR or requirement citation

If a reviewer claims any retained advisory/session surface is intentional, the
review must answer all of these points directly:

- why `PostSendHookEmitter` plus the receiver-specific handoff seam is
  insufficient
- why the shared dispatcher boundary must own a receiver-specific stream API
- why daemon-owned graft session maps and per-session queues are fundamental to
  ATM rather than implementation detail
- why the local IPC receive loop must do more than read, dispatch, and return
  a typed response

Until that review says otherwise, the accepted working assumption for
`Phase AD` is that every advisory/session surface above is removal scope.

## Follow-On Planning Artifacts

The corrective implementation line for this inventory is planned in:

- [Sprint AD.12](./sprint-AD12.md)
- [Sprint AD.13](./sprint-AD13.md)
- [Sprint AD.14](./sprint-AD14.md)
- [Sprint AD.15](./sprint-AD15.md)
- [Sprint AD.16](./sprint-AD16.md)
