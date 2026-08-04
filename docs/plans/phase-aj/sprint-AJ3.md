---
id: AJ.3
title: CLI Wire Payload Integration
status: planned
branch: feature/pAJ-s3-cli-wire-payload
worktree: ../atm-core-worktrees/feature/pAJ-s3-cli-wire-payload
target: integrate/phase-AJ
---

# Sprint AJ.3 — CLI Wire Payload Integration

## Goal

Carry optional `ActivityObservation` on `WriteRequest` and `ReadQuery`, then
have `atm send`, `atm read`, `atm ack`, and graft populate it from their
environment-derived context so the daemon sees caller observational state on every
local dispatch (UDS or TCP — both share the same wire structs).

Before attaching the new field, decompose `send/mod.rs` below the repository's
1,000 non-test-line ceiling. This is an implementation precondition, not a
post-hoc cleanup: move the existing request-construction/dispatch helpers into
`crates/atm-core/src/send/request.rs` (and update `send/mod.rs` re-exports)
without behavior change, then add `activity_observation` at the resulting
stable seam.

Apply the same precondition to `read/mod.rs`: before adding
`ReadQuery.activity_observation`, extract existing query-construction helpers
into `crates/atm-core/src/read/request.rs` and reduce `read/mod.rs` to at most
1,000 non-test lines. Remove its current `Q.6 split pending` line-count
exclusion from `.just/lint-config.toml` in the same decomposition commit; AJ.3
must not use that unscoped exclusion to absorb new observation wiring.

## Hard Dependencies

- AJ.1 and AJ.2 merged forward into this branch
- `integrate/phase-ai-31-33 @ 150391ecdf2e003185bff7d78427cd21509a7981`
- Completed Phase-AI reconciliation gate; `integrate/phase-AJ` was cut from
  the recorded post-merge `develop` SHA before AJ.1 and AJ.3 begin
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/send/mod.rs` baseline
- `crates/atm-core/src/read/mod.rs` baseline

## Dependency Relation

- `must_follow` AJ.2 because CLI and graft transmit its attestation DTO.
- No AJ sprint is `parallel_safe`: AJ.4 consumes these wire fields. AJ.3 begins
  immediately after AJ.2 → AJ.3 merge-forward; it does not wait for AJ.2 QA.
  Repeat that merge before every AJ.3 dev/fix round; AJ.3's PR completes after
  AJ.2's PR merges.
- On AJ.3 development-head push, AJ.4 begins immediately by merging
  AJ.3 → AJ.4; AJ.4 must complete that merge before any dev/fix round and does
  not wait for AJ.3 QA.

## Exact Targets

- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/send/request.rs` (new decomposition module)
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/read/request.rs` (new decomposition module)
- `.just/lint-config.toml` (remove the `read/mod.rs` exclusion after the split)
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm/src/commands/send.rs`
- `crates/atm/src/commands/read.rs`
- `crates/atm/src/commands/ack.rs`
- `crates/atm-graft-python/src/lib.rs`
- `crates/atm-daemon/src/https_transport.rs` (remote-ingress stripping only)

No changes to `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
or `crates/atm-daemon/src/local_tcp_transport.rs` — the unified HTTP
framing passes the JSON body through unchanged.

## Interfaces To Add Or Modify

- `WriteRequest` and `ReadQuery` gain
  `pub activity_observation: Option<ActivityObservation>` with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`.
  It is transient request metadata, consumed only by the trusted observation
  merge after successful local dispatch; it must not enter a `mail_messages`
  row or message payload.
- `WriteRequest::with_activity_observation(Option<ActivityObservation>) -> Self`
  and `ReadQuery::with_activity_observation(Option<ActivityObservation>) -> Self`
  are the sole builder-style setters. They only attach the additive DTO; they
  do not validate it, persist it, or affect command behavior.
- `SendCommand::build_request()` and the `read` command call the respective
  setter with the optional observation from `CallerContext`.
- `AckRequest` gains the same optional field. `atm ack` copies it from
  `CallerContext`; both `AckRequest::into_write_request()` and
  `AckRequest::from_unresolved_write()` preserve it. This is required because
  acknowledgements become the canonical `WriteRequest` only through that
  conversion.
- `PyGraftSession` retains its existing caller argument for normal graft
  behavior, but read/send/ack obtain observation only through AJ.2's shared
  resolver against that resolved caller. Args-only graft sessions and an
  environment mismatch serialize no observation; the Python address is never
  copied directly into telemetry.
- HTTPS peer ingress uses one explicit
  `clear_remote_activity_observation(&mut ApiRequest)` helper before the
  shared router. It clears the DTO from both `ApiRequest::Write` and
  `ApiRequest::Messages(MessageCollectionRequest::Receive)`; remote peer
  traffic can never update local observation.

## Deliverables

- `send/mod.rs` is at or below 1,000 non-test lines before the observation
  field/builder is added; its extracted request helpers retain existing public
  API behavior and tests. AJ.3 must not waive the line-count lint or add an
  exclusion for this module.
- `read/mod.rs` is at or below 1,000 non-test lines before the `ReadQuery`
  observation field/builder is added; its extracted request helpers retain
  existing public API behavior and tests, and its `Q.6 split pending`
  line-count exclusion is removed rather than extended or re-justified.
- CLI and graft transmit `activity_observation` only when a trusted environment
  identity/team attests the caller; otherwise the field is omitted entirely
  (no `null` literals, no empty strings)
- Wire payloads remain readable by older daemon builds — the new fields
  are strictly additive
- Payloads flow identically over UDS and TCP because both transports use
  `HttpFrameReader` and dispatch through the same `ApiRouter`
- No CLI flag is added in this sprint; values come from env only
- No code path inspects observation telemetry to alter CLI/graft behavior

## Required Validation

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p atm-core -p atm -p atm-graft-python -p atm-daemon`
- `just lint` proves `send/mod.rs` remains under the production-line ceiling
- `just lint` proves `read/mod.rs` remains under the production-line ceiling
  after its exclusion is removed
- New integration test: serialize `WriteRequest`/`ReadQuery` with observation
  `None` → JSON contains no `activity_observation`; with `Some(...)` → JSON
  contains the nested team/member/session/pid values
- Manual smoke over UDS: `ATM_SESSION_ID=smoke-1 ATM_PID=$$ atm send ...`
  against a dev daemon and confirm receipt in daemon trace log
- Manual smoke over TCP loopback: same env against a daemon started with
  TCP enabled; confirm identical receipt in trace log (UDS/TCP parity)
- Regression test: an inbound HTTPS `WriteRequest` with a forged observation
  reaches normal write handling with the DTO stripped. The matching forged
  remote `Receive(ReadQuery)` case also reaches normal handling with the DTO
  stripped. AJ.4 proves neither ingress can update `RuntimeStatusCache`.
- Ack conversion test: an environment-attested `AckRequest` becomes one
  `WriteRequest` with identical `activity_observation`; converting that write
  back through `from_unresolved_write()` preserves the same DTO.
- Storage regression test: serializing the durable mail row/payload after a
  local write proves it contains no `activity_observation`, session id, or pid
  telemetry from the request DTO.
- `rg -n "activity_observation" crates/atm-core/src/send/mod.rs crates/atm-core/src/read/mod.rs crates/atm-core/src/ack/mod.rs crates/atm-daemon/src/https_transport.rs`
  shows the additive field, acknowledgement conversion, and remote-ingress
  clearing boundary
- `git diff --check`

## Acceptance Criteria

- request-capture tests prove CLI and graft send/read/ack only transmit
  telemetry when env identity/team match any corresponding arguments; UDS/TCP
  use identical DTOs, both remote Write and Receive ingress strip it, and
  remote HTTPS cannot carry observation into the cache.
- AJ.3 must_follow AJ.2 under the merge-forward and PR-completion rule in the
  phase plan.
