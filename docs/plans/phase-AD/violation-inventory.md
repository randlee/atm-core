---
title: Phase AD Boundary Reset Violation Inventory
status: active
branch: plan/daemon-graft-boundary-reset
worktree: /Users/randlee/Documents/github/atm-core-worktrees/plan/daemon-graft-boundary-reset
---

# Phase AD Boundary Reset Violation Inventory

## Purpose

This document records the boundary drift that still blocks `Phase AD` release
readiness after the original `AD.1` through `AD.11` line.

It is not a speculative cleanup list. Each item below identifies concrete code
or doc surface that currently either:

- treats graft-specific session, queue, or stream behavior as shared
  daemon/core infrastructure
- preserves UUID-based retained ATM message identity or UUID-specific
  compatibility code even though the accepted line is now ULID-only
- allows raw CLI command behavior to diverge by worktree/invocation directory
  so one `ATM_HOME` install can silently bypass the canonical daemon/SQLite
  path
- returns internally inconsistent `atm read` mutation output by mixing the
  original selected message id with the next unread payload and stale
  pre-mutation bucket counts

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
| `crates/atm-core/src/protocol.rs` | `RequestEnvelope`, `ResponseEnvelope`, and related shared envelope metadata still reserve advisory register/unregister/fetch/drain/stream families even though those packet types are only for one receiver implementation. | Delete the graft-specific advisory variants from the shared protocol surface in the same closure that removes the shared advisory DTOs. |
| `crates/atm-daemon-client/src/wire.rs` | `MessageKind` reserves first-class daemon packet families for `AdvisoryRegister`, `AdvisoryUnregister`, `AdvisoryFetch`, `AdvisoryDrain`, and `AdvisoryStream`. | Remove graft-only advisory packet kinds from the accepted daemon wire registry. |
| `crates/atm-daemon/src/runtime_health.rs` | `DaemonRequestDispatcher` routes `RequestEnvelope::AdvisoryRegister`, `AdvisoryUnregister`, `AdvisoryFetch`, `AdvisoryDrain`, and implements `dispatch_advisory_stream(...)`. | Remove graft-specific routing from the daemon dispatcher. The dispatcher must not own one receiver implementation's session protocol. |
| `crates/atm-daemon/src/advisory_runtime.rs` | `AdvisoryRuntime` owns graft session registration, per-session nudge queues, dropped-count bookkeeping, and stream loop behavior. | Delete daemon-owned graft session runtime state and direct callers. |
| `crates/atm-daemon/src/local_ipc_transport/request_worker.rs` | The local IPC worker special-cases `RequestEnvelope::AdvisoryStream` and owns a dedicated stream sink path. | Remove receiver-specific streaming logic from the transport worker so the receive loop returns to thin framed unary dispatch. |
| `crates/atm-daemon/src/tests_advisory.rs` and advisory-specific test seams | Daemon tests currently normalize daemon-owned graft session registration, fetch/drain, and stream behavior as core runtime obligations. | Remove the advisory-runtime test lane and replace it with tests that cover the accepted post-send seam only. |
| `crates/atm/src/composition.rs` | The production CLI composition layer still implements `AdvisorySessionPort` and exposes `register_graft_session(...)`, `unregister_graft_session(...)`, `fetch_graft_nudges(...)`, and `drain_graft_nudges(...)`, leaking the shared advisory contract into the retained CLI binary. | Delete the production advisory trait implementation and helper methods in the same closure that deletes the shared advisory boundary. |
| `crates/atm-graft/src/lib.rs`, `runtime.rs`, `transport.rs` | `atm-graft` is coupled to daemon-owned advisory registration, fetch/drain, and dedicated advisory-stream transport. | Reset `atm-graft` to a thin receiver implementation that owns any remaining receiver-side state internally and no longer depends on shared advisory session protocol families. |
| `Cargo.toml`, `Cargo.lock`, `crates/atm-core/Cargo.toml`, `crates/atm-core/src/schema/inbox_message.rs`, `crates/atm-core/src/mailbox/mod.rs`, `crates/atm-core/src/read/mod.rs`, `crates/atm-core/src/persistence.rs`, `crates/atm-core/tests/mailbox_locking.rs`, `crates/atm-storage/Cargo.toml`, `crates/atm-storage/src/schema/inbox_message.rs`, `crates/atm-storage-rusqlite/src/writer/ops.rs`, and `tools/schema_models/*` | Retained ATM code still depends on `uuid` and still accepts, emits, type-checks, or generates UUID values through `AtmMessageId::{from_uuid_wire, into_uuid_wire}`, UUID parse fallback, UUID-based serializers, UUID-backed test helpers, and UUID uniqueness helpers even though Claude JSON compatibility is retired and the accepted runtime is ULID-only. | Remove all retained UUID usage and make the accepted ATM line ULID-only across message identity, schema/tooling, tests, and supporting uniqueness helpers. |
| `crates/atm/src/composition.rs`, `crates/atm/src/commands/{send,read,ack,list,clear,members,teams,doctor}.rs`, `crates/atm-core/src/home.rs`, and the raw CLI smoke coverage | Raw CLI behavior can diverge by invocation directory/worktree: the same installed `atm` binary can emit unreadable compatibility-only sends from one worktree while persisting ULID/SQLite-backed sends from another, making wrapper-forced `cwd` normalization a correctness crutch. | Make retained raw CLI commands derive daemon socket, launch gate, and durable store roots from accepted ATM home resolution only; constrain invocation-directory use to config ingress, hook relative paths, and file-policy checks; add raw multi-worktree smoke coverage so wrapper-free CLI behavior is release-gated. |
| `crates/atm-core/src/read/mod.rs`, `crates/atm-core/src/read/metadata_selection.rs`, and read-mutation smoke/test coverage | After `atm read --unread` marks a message read, the response can still report the original `selected_message_id` while returning the next unread message payload and the pre-mutation unread counts. The durable write occurs, but the reported read result is not self-consistent. | Preserve the mutated message as the returned payload (or return no payload if that is the deliberate contract), and always return post-mutation bucket counts that correspond to the returned state. |

## Architecture And Requirements Drift

These docs currently bless the same leak and therefore must be corrected as
part of the boundary-reset line:

| Document | Current drift |
|---|---|
| `docs/atm-daemon/requirements.md` | Declares `advisory register`, `advisory unregister`, `advisory fetch`, `advisory drain`, and `advisory stream` as daemon packet families and states that one live advisory stream per active embedded client session is a production requirement. |
| `docs/atm-daemon/architecture.md` | States that the accepted daemon surface includes advisory register/unregister/fetch/drain/stream and that the daemon may own one bounded pending advisory queue plus one live stream per active session. |
| `docs/atm-daemon/protocol-icd.md` | Documents advisory register/unregister/fetch/drain/stream as first-class public packet kinds and envelope mappings. |
| `docs/atm-graft/architecture.md` | Requires a dedicated daemon advisory-stream connection, daemon-owned bounded pending-nudge state, and explicit graft session lifecycle states owned around that daemon session model. |
| `docs/atm-graft/requirements.md` | Carries the same daemon-owned persistent receive-loop and bounded queue assumptions into the published graft requirements surface. |
| `docs/atm-graft/boundaries.md` | Defines the session runtime consumer around a persistent receive thread and dedicated advisory-stream connection. |
| `docs/atm-core/requirements.md` | Reserves `AdvisorySessionId` and shared advisory packet kinds in `atm-core` requirements, boxing the shared boundary into the leaked session model. |
| `docs/requirements.md`, `docs/architecture.md`, `docs/atm-core/architecture.md`, and `docs/adr/ADR-012-one-message-identity.md` | Still describe UUID-wire message ids or UUID-based retained uniqueness rules as accepted ATM behavior even though Claude backend compatibility is retired and retained ATM runtime should now be ULID-only. |
| `docs/requirements.md`, `docs/architecture.md`, `docs/atm-core/requirements.md`, `docs/atm-core/architecture.md`, `docs/atm-daemon/requirements.md`, and `docs/atm-daemon/architecture.md` | Do not state strongly enough that invocation directory and worktree root are never selectors for daemon socket, launch-gate, or durable SQLite root selection, leaving wrapper-only `cwd` forcing as an accidental operational requirement. |
| `docs/requirements.md`, `docs/architecture.md`, `docs/atm-core/requirements.md`, and `docs/atm-core/architecture.md` | Do not currently state the output consistency rule for read-side state mutation, leaving it ambiguous whether `atm read --unread` should return the mutated message or the next unread message after mutation. |
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
`Phase AD` is that every advisory/session surface above and every retained
UUID surface above are removal scope.

## Follow-On Planning Artifacts

The corrective implementation line for this inventory is planned in:

- [Sprint AD.12](./sprint-AD12.md)
- [Sprint AD.13](./sprint-AD13.md)
- [Sprint AD.14](./sprint-AD14.md)
- [Sprint AD.15](./sprint-AD15.md)
- [Sprint AD.16](./sprint-AD16.md)
- [Sprint AD.17](./sprint-AD17.md)
- [Sprint AD.18](./sprint-AD18.md)
- [Sprint AD.19](./sprint-AD19.md)

Final closure ownership is:

- `AD.14` for shared advisory boundary removal, including
  `crates/atm-core/src/protocol.rs`, `crates/atm/src/composition.rs`, and
  `boundaries/atm-daemon-client/rpc-envelope.toml`
- `AD.15` for daemon runtime deletion and final daemon-doc closure
- `AD.16` for `atm-graft` runtime deletion and final graft-doc closure
- `AD.17` for verification/readiness only
