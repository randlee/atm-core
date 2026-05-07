# Phase R.16 — Peer Delivery And Replay

```yaml
plan_type: sprint_plan
phase: R
sprint: "R.16"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pR-s16-peer-replay
branch: feature/pR-s16-peer-replay
status: complete
estimated_scope: M
```

## Goal

Replace the peer transport stub with a real daemon-to-daemon outbound path and wire crash-safe replay/re-export around it.

## Scope Summary

This sprint covers the outbound remote-delivery lane: peer framing, typed timeout/retry behavior, and the durable replay path that preserves SQLite commit ordering across crash/restart recovery.

## Governing Requirements

- `REQ-P-RUNTIME-002`
- replay and remote-delivery requirements in `docs/requirements.md`
- `REQ-RUSQLITE-STORE-001`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`

## Governing Boundaries

- `BOUNDARY-ClientTransport-Peer`
- `BOUNDARY-AtmProtocol`
- `BOUNDARY-MailStore-Sqlite`

## Prerequisites

- `R.13` runtime lifecycle is complete
- `R.14` host-scoped SQLite root is complete

## Hard Dependencies

- should land before any production release that claims remote daemon-to-daemon parity

## Non-Goals

- member heartbeat/status cache
- watch/reconcile runtime
- notifier runtime

## Sub-Tasks

1. Peer client transport
   Development work:
   - replace `PeerClientTransport::send()` stub with real outbound request framing
   - implement timeout/retry semantics for remote daemon acceptance
   - classify retryable vs non-retryable socket/network/TLS/protocol failures
   - define one typed `RemoteDeliveryOutcomeUnknown` path for drop-after-send /
     acceptance-unknown cases
   - keep the default total remote retry budget at `30s`, with one documented
     config override (`daemon.remote_retry_budget`) for operators who need a
     longer window
   - make outbound connect attempts and inbound listener behavior explicit
     under local interface churn and rebinding
   - keep remote transport on the shared ATM protocol rather than a side API
   Required tests:
   - request/response framing tests
   - retry/timeout tests with deterministic fake peer behavior
   - retry classification tests covering timeout/refused/reset/unreachable vs
     TLS/protocol/reject failures
   - unknown-outcome tests for disconnect-after-write-before-acceptance
   - listener rebinding tests for wildcard bind survival and explicit-address
     reload/degraded behavior
   Required doc or boundary updates:
   - update daemon transport architecture if any DTO or timeout names change

2. Durable replay / re-export
   Development work:
   - add persisted replay state keyed by `message_key`
   - wire startup crash recovery so committed local state can resume export/remote handoff safely
   - define bounded expiry and operator-visible failure/degraded behavior
   Required tests:
   - crash/restart replay tests
   - duplicate-delivery prevention tests across restart
   Required doc or boundary updates:
   - update replay/recovery sections of requirements and architecture

## Split Recommendation

Do not split peer transport from replay unless the replay state model is still blocked on SQLite schema work. A real peer path without crash-safe replay is not production-complete.

## Acceptance Criteria

- `PeerClientTransport::send()` no longer returns a stub error
- remote delivery uses the shared protocol framing and typed retry/timeout behavior
- retryable and non-retryable peer failures are documented and covered by
  deterministic tests
- `RemoteDeliveryOutcomeUnknown` exists as the typed result for
  drop-after-send / acceptance-unknown paths
- the default `30s` remote retry budget and its config override are documented
  and validated
- listener/interface rebinding behavior is explicit: wildcard binds survive
  ordinary interface churn without restart; explicit-address binds degrade and
  require reload/rebind when that address disappears
- replay state survives crash/restart and resumes pending export/remote work without duplicating committed local state
- operator-visible degraded/failure paths exist for replay exhaustion or remote unavailability

## Required Validation

- `cargo test -p atm-daemon`
- `cargo test --workspace`
- `just lint`

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/phase-R/issues.md`

## Risks And Watchouts

- durable replay must preserve local commit ordering; do not invent a best-effort outbox that can drift from SQLite truth
- peer transport should reuse the shared request/response envelopes rather than forking the protocol family
