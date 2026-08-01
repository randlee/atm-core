---
id: AJ.4
title: Daemon Cache Touch On Dispatch
status: planned
branch: feature/pAJ-s4-daemon-cache-touch
worktree: ../atm-core-worktrees/feature/pAJ-s4-daemon-cache-touch
target: integrate/phase-AJ
---

# Sprint AJ.4 — Daemon Cache Touch On Dispatch

## Goal

Teach the daemon to update `RuntimeStatusCache` as a side effect of every
send/read/ack dispatch, honoring the non-overwrite rule for absent
optional fields — with a single touch site in the shared dispatcher so
UDS and TCP both update the same cache entry.

## Hard Dependencies

- AJ.1, AJ.2, and AJ.3 merged forward into this branch
- `integrate/phase-AJ` baseline (unified HTTP-framed local transport:
  UDS in `local_ipc_transport/request_worker.rs`, TCP in
  `local_tcp_transport.rs`, both dispatching into `ApiRouter`)
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-daemon/src/runtime_health.rs` baseline
- `crates/atm-daemon/src/runtime_status_cache.rs` baseline

## Exact Targets

- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-daemon/src/runtime_health.rs`

Explicitly NOT touched (framing is transport-agnostic and stays that way):

- `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
- `crates/atm-daemon/src/local_tcp_transport.rs`

## Interfaces To Add Or Modify

- Cache entry struct gains `pub session_id: Option<SessionId>` and
  `pub pid: Option<u32>`
- New public method on the cache:
  `pub fn touch_member(&self, identity: &str, session_id: Option<SessionId>, pid: Option<u32>)`
  implementing the non-overwrite rule:
  - if `session_id` is `Some`, replace cached value
  - if `session_id` is `None`, leave cached value untouched
  - same for `pid`
- New public accessor
  `pub fn cached_session_id(&self, identity: &str) -> Option<SessionId>`
  — returns the currently cached `session_id` for the identity, or
  `None` if the member has no cache entry or no `Some` value has ever
  been written. Infallible: no locking failure is surfaced to callers
  (poisoned-lock handling follows the cache's existing convention); it
  never errors.
- `route_write()` in `runtime_health.rs` calls `touch_member` after a
  successful dispatch with the caller identity and the wire-supplied
  `session_id` / `pid`
- `dispatch_non_write()` for the `Receive` path calls `touch_member` with
  the same arguments sourced from `ReadQuery`

Because UDS (`request_worker.rs`) and TCP (`local_tcp_transport.rs`) both
land in `ApiRouter` and both go through `route_write()` /
`dispatch_non_write()`, these two call sites cover both transports with
no per-transport code.

## Deliverables

- Cache entries persist `session_id` and `pid` across calls
- A dispatch carrying `Some(...)` values updates the cache
- A dispatch carrying `None` values leaves the existing cached values
  untouched (covered by dedicated tests — one `Some` followed by one
  `None` followed by an accessor read must return the original `Some`)
- The same test runs once over UDS and once over TCP loopback and
  produces identical cache state (transport parity)
- Touching the cache is a side effect only; dispatch behavior is
  unchanged regardless of whether the fields are present
- Failure to update the cache must not fail the dispatch — errors are
  logged at `warn!` and swallowed

## Required Validation

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p atm-daemon`
- New unit tests in `runtime_status_cache.rs`:
  - `touch_member_some_then_none_preserves_value`
  - `touch_member_none_on_empty_cache_stays_none`
  - `touch_member_some_overwrites_some`
  - same trio for `pid`
- New integration test in `runtime_health.rs` exercises send → read →
  ack and asserts the cache reflects the latest `Some` values
- New transport-parity integration test: same dispatch sequence issued
  once via the UDS path and once via the TCP loopback path against the
  same daemon; assert identical `RuntimeStatusCache` contents afterwards
- `rg -n "touch_member|cached_session_id" crates/atm-daemon/src/` shows
  the new surface; hits appear in `runtime_health.rs` and
  `runtime_status_cache.rs` only — never in `local_ipc_transport/` or
  `local_tcp_transport.rs`
- `git diff --check`

## Acceptance Criteria

- cache touch occurs exactly once only after successful send/read/ack and only
  for a trusted observation; it never changes state-machine behavior.
- AJ.4 must_follow AJ.3 under the merge-forward and PR-completion rule in the
  phase plan.
