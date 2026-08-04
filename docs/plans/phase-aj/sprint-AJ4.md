---
id: AJ.4
title: Daemon Cache Touch On Dispatch
status: planned
branch: feature/pAJ-s4-daemon-cache-touch
worktree: ../atm-core-worktrees/feature/pAJ-s4-daemon-cache-touch
target: integrate/phase-aj
---

# Sprint AJ.4 — Daemon Cache Touch On Dispatch

## Goal

Teach the daemon to update `RuntimeStatusCache` as a side effect of every
send/read/ack dispatch, honoring the non-overwrite rule for absent
optional fields — with a single touch site in the shared dispatcher so
UDS and TCP both update the same cache entry.

Before extending dispatcher behavior, run the production-line checkpoint for
`runtime_health.rs`. If it is above 900 non-test lines, first extract the
existing observation-free route helpers into
`crates/atm-daemon/src/runtime_health/dispatch.rs`, preserving behavior and
tests; do not add AJ surface to a file that would exceed the 1,000-line ceiling.

## Hard Dependencies

- AJ.1, AJ.2, and AJ.3 merged forward into this branch
- `integrate/phase-ai-31-33 @ 150391ecdf2e003185bff7d78427cd21509a7981`:
  unified HTTP-framed local transport, UDS in
  `local_ipc_transport/request_worker.rs` and TCP in
  `local_tcp_transport.rs`, both dispatching into `ApiRouter`
- Completed Phase-AI reconciliation gate; `integrate/phase-aj` was cut from
  the recorded post-merge `develop` SHA before AJ.1 and AJ.4 begin
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-daemon/src/runtime_health.rs` baseline
- `crates/atm-daemon/src/runtime_status_cache.rs` baseline

## Dependency Relation

- `must_follow` AJ.3 because cache touch consumes its optional wire DTO.
- No AJ sprint is `parallel_safe`: AJ.5 reuses this merge contract and AJ.6
  projects its cache fields. AJ.4 begins immediately after AJ.3 → AJ.4
  merge-forward; it does not wait for AJ.3 QA. Repeat that merge before every
  AJ.4 dev/fix round; AJ.4's PR completes after AJ.3's PR merges.
- On AJ.4 development-head push, AJ.5 begins immediately by merging
  AJ.4 → AJ.5; AJ.5 must complete that merge before any dev/fix round and does
  not wait for AJ.4 QA.

## Exact Targets

- `crates/atm-core/src/protocol.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_health/dispatch.rs` (split contingency when
  the checkpoint requires it)
- `crates/atm-daemon/src/https_transport.rs` (remote ingress stripping test
  coverage only; AJ.3 owns the stripping helper)

Explicitly NOT touched (framing is transport-agnostic and stays that way):

- `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
- `crates/atm-daemon/src/local_tcp_transport.rs`

## Interfaces To Add Or Modify

- Add public protocol:
  ```rust
  pub enum RuntimeObservationSource {
      Heartbeat,
      LocalCommand,
  }
  ```
  It derives `Debug`, `Clone`, `Copy`, `Serialize`, `Deserialize`,
  `PartialEq`, and `Eq`.
  It identifies ingress provenance; it is not an authority or behavior
  selector. Graft's environment-attested read/send/ack uses `LocalCommand`;
  AJ does not add a graft-specific request field merely for provenance.
- `RuntimeMemberRecord` gains `session_id: Option<SessionId>`, `pid: Option<u32>`,
  `state_changed_by: Option<RuntimeObservationSource>`,
  `state_changed_at: Option<IsoTimestamp>`,
  `session_changed_by: Option<RuntimeObservationSource>`, and
  `session_changed_at: Option<IsoTimestamp>`, while retaining its existing
  `last_active_at`. State provenance changes only on a lifecycle edge; session
  provenance changes only on a session edge.
- `RuntimeMemberRecord.state_changed_at: Option<IsoTimestamp>` updates only
  for a real lifecycle transition, never for a metadata-only or same-state
  activity touch.
  `Active → Active`, `Idle → Idle`, and `Offline → Offline` are not edges.
  Every lifecycle value records its first entry and retains that timestamp until
  a different-state edge.
- `last_active_at` advances on each trusted `Active` observation but is not a
  state-edge timestamp and is never shown as the roster's state age.
- Every actual session/pid mutation emits one structured info event with id
  `runtime_observation_metadata_changed`, previous/new value, team/member,
  source, and timestamp; no-op input emits no change event.
- Add one crate-private, infallible merge helper:
  ```rust
  fn merge_observation(
      &self,
      key: &RuntimeMemberKey,
      source: RuntimeObservationSource,
      state: RuntimeMemberState,
      session_id: Option<&SessionId>,
      pid: Option<u32>,
      observed_at: IsoTimestamp,
  ) -> ObservationMergeOutcome
  ```
  It is the only AJ mutation point for `RuntimeMemberRecord` observation fields.
  `touch_member` and `record_heartbeat` are thin ingress adapters; no trait or
  transport-specific implementation is introduced.
- Add crate-private, copyable `ObservationMergeOutcome` with at least
  `pid_changed`, `session_changed`, and `state_changed`. It is returned by the
  merge helper so heartbeat response construction and audit emission share one
  comparison result. `pid_changed` is true only for replacement of one defined
  pid by a different defined pid; initial `None -> Some(pid)` is an audited
  mutation but reports `false` for compatibility with the existing response
  field's replacement meaning.
  ```rust
  struct ObservationMergeOutcome {
      pid_changed: bool,
      session_changed: bool,
      state_changed: bool,
  }
  ```
- Add daemon-private `TrustedActivityObservation(ActivityObservation)`. Only
  `ApiRouter`'s successful local `AuthenticatedIngress` dispatch path may
  construct it, through a private constructor that consumes the local-ingress
  proof and an optional request DTO. `touch_member` accepts this capability,
  never raw `ActivityObservation`; peer, anonymous, and HTTPS-stripped paths
  cannot type-check a cache mutation call.
- New crate-private method on the cache:
  `pub(crate) fn touch_member(&self, observation: &ActivityObservation,
  observed_at: IsoTimestamp)`
  implementing latest-trusted-observation semantics:
  - if `observation.session_id` is `Some`, replace the cached current value
  - if it is `None`, leave the cached current value untouched
  - blank/whitespace session values normalize to absent before these rules
  - same for `observation.pid`; heartbeat's required pid overwrites through the
    shared helper
  - a different pid/session emits one retained change event but never rejects
    ingress, changes lifecycle state, or selects behavior
- AJ adds no reset API. Roster removal drops the entry; a future re-add starts
  with no observation. `touch_member` must not write `Unknown` or clear a
  defined session.
- New crate-private accessor
  `pub(crate) fn cached_session_id(&self, team: &TeamName, member: &AgentName) -> Option<SessionId>`
  — returns the currently cached `session_id` for the identity, or
  `None` if the member has no cache entry or no `Some` value has ever
  been written. Infallible: `cached_session_id` reads through the existing
  ArcSwap snapshot-load pattern (no `Mutex`/`RwLock`, no poisoning possible);
  it never errors.
- Thread `AuthenticatedIngress` from `ApiRouter::route()` through
  `dispatch_with_deadline()`, `route_write()`, and `dispatch_non_write()`.
  These functions use it only to enforce the local-observation trust boundary;
  it is not session/pid/state policy.
- `route_write()` calls `touch_member` exactly once, after `finish()` reports a
  successful write, only when ingress is `Local`, and with the pre-write
  `WriteRequest.activity_observation`. A failed persistence or post-write
  route never touches the cache.
- The shared `dispatch_with_deadline()` guard remains outside the observation
  merge. An expiry before route dispatch produces no observation. If an
  already-admitted local write/read has completed its normal dispatch work and
  the guard subsequently returns the existing retry-safe
  `ATM_DAEMON_MAY_HAVE_EXECUTED` outcome, the accepted local observation is
  retained: it describes the daemon-side event, not a client-visible success.
  Observation must never relabel that timeout outcome or add a retry decision.
- `dispatch_non_write()` for the `Receive` path calls `touch_member` exactly
  once after a successful read, only when ingress is `Local`, and with
  `ReadQuery.activity_observation`. A failed read never touches the cache.
- Local ingress supplies `IsoTimestamp::now()` at successful daemon dispatch;
  heartbeat supplies its accepted `request.observed_at`, matching the existing
  heartbeat contract. Merge order is accepted-ingress order: AJ adds no
  stale-event rejection, clock-skew policy, or state-based retry behavior.

Because UDS (`request_worker.rs`) and TCP (`local_tcp_transport.rs`) both
land in `ApiRouter` and both go through `route_write()` /
`dispatch_non_write()`, these two call sites cover both transports with
no per-transport code.

## Deliverables

- Cache entries retain `session_id` and `pid` for the daemon process lifetime
- A dispatch carrying `Some(ActivityObservation { .. })` updates the cache
- A dispatch carrying `None` leaves the existing cached values
  untouched (covered by dedicated tests — one `Some` followed by one
  `None` followed by an accessor read must return the original `Some`)
- A local CLI/graft touch after a heartbeat-derived state transitions it to
  `Active` with `LocalCommand` provenance and preserves absent session metadata.
- The same test runs once over UDS and once over TCP loopback and
  produces identical cache state (transport parity)
- Touching the cache is a side effect only; dispatch behavior is
  unchanged regardless of whether the fields are present
- Cache merging is an infallible side effect; it never changes a successful
  dispatch result.

## Required Validation

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p atm-daemon`
- `just lint` proves `runtime_health.rs` remains at or below the production
  line ceiling after the checkpoint/split
- New unit tests in `runtime_status_cache.rs`:
  - `touch_member_some_then_none_preserves_value`
  - `touch_member_none_on_empty_cache_stays_none`
  - `touch_member_some_overwrites_some`
  - `blank_session_normalizes_to_absent_without_clearing_known_session`
  - same trio for `pid`
  - `normal_updates_never_regress_known_state_or_session_to_default`
  - `roster_removal_drops_observation_and_readd_starts_unknown`
  - `unknown_and_offline_are_distinct_states_with_distinct_provenance`
  - `trusted_cli_activity_transitions_offline_or_idle_to_active`
  - `state_changed_at_updates_only_on_real_state_transition`
  - `state_changed_by_updates_only_on_real_state_transition`
  - `session_changed_by_and_at_update_only_on_session_edge`
  - `session_and_pid_mutations_emit_exactly_one_audit_event`
  - `pid_changed_response_is_false_for_initial_set_and_true_for_replacement`
  - `changed_pid_or_session_is_retained_evidence_not_identity_conflict`
  - `no_op_metadata_updates_emit_no_audit_event`
- New integration test in `runtime_health.rs` exercises send → read → ack and
  asserts the cache reflects the latest attested observation
- Regression tests prove a failed local send/read, and peer, untrusted-smoke,
  and anonymous ingress carrying a forged observation, cannot mutate the
  cache. This remains true if an HTTPS stripping regression is introduced:
  dispatcher ingress gating is defense in depth, not a behavior policy.
- Compile-time coverage proves only the private local-dispatch constructor can
  produce `TrustedActivityObservation`; `touch_member` has no raw
  `ActivityObservation` overload.
- Regression tests prove an expired pre-dispatch deadline leaves the cache
  untouched, while a post-side-effect deadline result retains the already
  accepted local observation and returns the existing retry-safe uncertainty
  unchanged.
- New transport-parity integration test: same dispatch sequence issued
  once via the UDS path and once via the TCP loopback path against the
  same daemon; assert identical `RuntimeStatusCache` contents afterwards
- `rg -n "touch_member|merge_observation|cached_session_id" crates/atm-daemon/src/`
  shows the new surface in `runtime_health.rs` and `runtime_status_cache.rs`
  only — never in `local_ipc_transport/` or `local_tcp_transport.rs`
- `git diff --check`

## Acceptance Criteria

- cache touch occurs exactly once only after successful send/read/ack and only
  for a trusted observation; it never changes state-machine behavior.
- AJ.4 must_follow AJ.3 under the merge-forward and PR-completion rule in the
  phase plan.
