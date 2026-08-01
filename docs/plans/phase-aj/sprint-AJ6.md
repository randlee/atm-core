---
id: AJ.6
title: Snapshot Surface And Integration Validation
status: planned
branch: feature/pAJ-s6-snapshot-and-validation
worktree: ../atm-core-worktrees/feature/pAJ-s6-snapshot-and-validation
target: integrate/phase-AJ
---

# Sprint AJ.6 — Snapshot Surface And Integration Validation

## Goal

Surface current observation on `RuntimeStatusSnapshot` and `atm members`, then
run the full-workspace validation that closes Phase AJ.

## Hard Dependencies

- AJ.1 through AJ.5 merged forward into this branch
- `integrate/phase-AJ` at the Phase AJ entry-gate SHA
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`

## Dependency Relation

- `must_follow` AJ.5 because this sprint exposes the final converged cache
  shape and heartbeat semantics.
- No AJ sprint is `parallel_safe`: AJ.6 is the phase closeout. Merge AJ.5 →
  AJ.6 before every dev/fix round; AJ.6's PR completes after AJ.5's PR, while
  development may start after the AJ.5 development commit is pushed.

## Exact Targets

- `crates/atm-core/src/protocol.rs` (`RuntimeStatusSnapshot` per-member
  entry)
- `crates/atm-daemon/src/runtime_status_cache.rs` (`snapshot()` builder)
- `crates/atm/src/commands/members.rs` and its existing output projection
- `boundaries/atm-daemon/daemon-status-source.toml` (non-authoritative
  runtime-observation boundary record)
- `docs/atm-daemon/boundaries.md` (matching human boundary contract)
- `.just/tests/test_runtime_observation_boundary.py` (narrow source-use gate)
- `docs/requirements.md` (`REQ-CORE-RUNTIME-002` / `-004`)
- `docs/adr/ADR-045-runtime-observation-attribution.md`
- `docs/architecture.md` (runtime-health observation boundary)
- `docs/team-member-state.md` (verify the planned observational contract
  matches the implemented public surface; make only implementation-driven
  precision corrections)
- `docs/plans/phase-aj/plan-phase-aj.md` (exit-criteria checkboxes)
- Sprint frontmatter status flips on AJ.1 through AJ.6

## Interfaces To Add Or Modify

- Add public `RuntimeMemberObservation`, deriving `Debug`, `Clone`,
  `Serialize`, `Deserialize`, `PartialEq`, and `Eq`:
  ```rust
  pub struct RuntimeMemberObservation {
      pub team: TeamName,
      pub member: AgentName,
      pub state: RuntimeMemberState,
      pub session_id: Option<SessionId>,
      pub pid: Option<u32>,
      pub last_active_at: Option<IsoTimestamp>,
      pub state_changed_by: Option<RuntimeObservationSource>,
      pub state_changed_at: Option<IsoTimestamp>,
      pub session_changed_by: Option<RuntimeObservationSource>,
      pub session_changed_at: Option<IsoTimestamp>,
  }
  ```
  Optional fields use `#[serde(default, skip_serializing_if =
  "Option::is_none")]`; structured output contains raw session id and pid.
- `RuntimeStatusSnapshot` gains `members: Vec<RuntimeMemberObservation>` with
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
- Update `BOUNDARY-StatusSource-Daemon` to name runtime observation as
  non-authoritative telemetry and add the review gate
  `runtime_observation_non_authoritative`; update the matching daemon boundary
  narrative. This does not add a new I/O ownership tag or broaden the status
  adapter's dependencies.
- `RuntimeStatusCache::snapshot_for_members()` populates one observation per
  roster member, retaining `Unknown` in structured output so counts and state
  are unambiguous. `atm members` filters default observation from human output.
- Snapshot and `atm members` render source/timestamp only beside defined state
  or session values; default `Unknown`/absent-session remains hidden.
- `state_changed_at` is rendered only beside a defined non-`Unknown` state.
- Human roster output renders state age from `state_changed_at` (for example,
  `Idle — 30m`); structured output retains the absolute timestamp. Fixture
  tests use a fixed clock, not wall-clock sleeps.
- A shortened human session is the first 12 Unicode scalar values followed by
  `…` only when longer; it must never expose the full value in a human table.
- `atm members` displays a member's observed state age, pid, and shortened
  session id only when defined. Default `Unknown` with no session/pid adds no
  field, line, or placeholder to existing roster output.

## Deliverables

- `RuntimeStatusSnapshot` includes current session/pid for every member that
  has one cached; absent values are omitted on the wire
- Snapshot wire format remains backward compatible — older consumers
  ignore the new field
- Full end-to-end integration test exercising:
  1. CLI `atm send` over UDS with environment-attested
     `ATM_SESSION_ID=s-end` and `ATM_PID=4242`
  2. CLI `atm read` over UDS with the same env
  3. HTTP heartbeat with `session_id: None`
  4. Snapshot read shows `session_id: Some("s-end")` and `pid: Some(4242)`
     for the identity
  5. CLI `atm ack` over TCP loopback with `ATM_SESSION_ID` unset
  6. Snapshot still shows `Some("s-end")` (the absent nested field preserves
     session across mixed paths and transports)
- Static boundary audit confirms `activity_observation` is merged only in
  `runtime_status_cache.rs`, is cleared in HTTPS peer ingress, and has no
  routing/nudge/retry/admission/delivery policy call sites.
- Roster fixture tests prove JSON preserves raw pid/session while human output
  shortens a session id and shows state age from a fixed clock.
- Backward-compatibility fixtures prove a new reader accepts a pre-AJ
  `RuntimeStatusSnapshot` with no `members` field and an older reader can
  ignore the new field.

## Required Validation

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo test -p atm-daemon snapshot_session_id`
- End-to-end test above passes on macOS and Linux CI
- New `.just/tests/test_runtime_observation_boundary.py` permits observation
  references only in the DTO/caller construction, local request construction,
  HTTPS stripping, `runtime_health.rs` forwarding, `runtime_status_cache.rs`
  merge/snapshot code, and roster projection. It rejects references in peer
  delivery, post-write routing, nudge, retry, admission, notification, and
  policy modules. This is a narrow source-use guard, not a behavior heuristic.
- Boundary-record regression test proves the named
  `runtime_observation_non_authoritative` review gate remains in
  `BOUNDARY-StatusSource-Daemon`.
- Grep audit:
  - `rg -n "activity_observation" crates/atm-core crates/atm-daemon crates/atm crates/atm-graft-python`
    returns only DTO construction, local dispatch/cache merge, and HTTPS
    clearing boundary hits
  - any routing/nudge/retry/admission/delivery use is a defect
- Transport audit: `rg -n "touch_member" crates/atm-daemon/src/` shows
  the call site inside `runtime_health.rs` only — no per-transport touch
  sites under `local_ipc_transport/` or in `local_tcp_transport.rs`
- All six sprint docs have `status: complete` in frontmatter
- `docs/team-member-state.md` agrees with the implemented closed ingress set,
  latest-accepted-trusted-observation behavior, and non-authoritative boundary;
  it must not reintroduce durable pid ownership, live-pid rejection, or an
  admin-assume-identity path.
- `REQ-CORE-RUNTIME-002` / `-004`, ADR-045, architecture, and the current
  team-member-state reference agree on the closed ingress set, no durable/mail
  telemetry, latest accepted trusted observation, and the no-business-logic
  boundary.
- `plan-phase-aj.md` exit criteria all checked off
- `git diff --check`

## Acceptance Criteria

- snapshot evolution is additive: retain `member_counts`, all current fields,
  and `CLI_SCHEMA_VERSION`; session and pid are optional additions only.
- `atm members` fixture tests prove default observations are omitted and a
  defined state/session/pid is rendered for the correct roster member.
- AJ.6 must_follow AJ.5 under the merge-forward and PR-completion rule in the
  phase plan.
