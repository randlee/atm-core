---
title: Phase AJ Plan
status: planned
branch: plan/phase-aj
worktree: ../atm-core-worktrees/plan/phase-aj
---

# Phase AJ Plan

## Phase-entry gate

Phase AJ begins only after every Phase AI change has merged. Team-lead records
the final AI cutover SHA on `develop`, creates `integrate/phase-AJ` at that
SHA, and includes it in every AJ dispatch. No AJ worktree, implementation, QA,
or merge-forward may use `integrate/phase-ai-31-33` as its base.

## Goal

Maintain runtime observational state for every roster member in the daemon's
`RuntimeStatusCache`, fed by two independent update paths that converge on a
single in-memory entry per roster member:

1. **CLI command path** — `atm send`, `atm read`, and `atm ack` carry optional
   `session_id` and `pid` fields on the existing local wire payload; the daemon
   touches the cache as a side effect of dispatch via `touch_member()` in
   `runtime_health.rs`.
2. **HTTP heartbeat path** — `POST /v1/atm/heartbeat` carries an optional
   `session_id`; the daemon records it through `record_heartbeat()` alongside
   the existing heartbeat activity.

State is in-memory only. No SQLite persistence. The fields are observational
only — recorded, never acted upon.

## Baseline

- **Target codebase: `integrate/phase-AJ`**, created from the recorded final
  AI cutover commit on `develop`. All AJ work merges forward from this line.
- Research: `docs/plans/phase-aj/phase-aj-research.md`
- Transport: UDS and TCP are unified under HTTP framing. Both read with
  `HttpFrameReader` and write with `write_local_http_response`; both dispatch
  into the same `ApiRouter`. Because the dispatcher is transport-agnostic, a
  single `touch_member()` call inside the dispatch path covers UDS and TCP
  with no transport-specific code.

  | Transport | File | Reader | Writer |
  |---|---|---|---|
  | UDS | `crates/atm-daemon/src/local_ipc_transport/request_worker.rs` | `HttpFrameReader` | `write_local_http_response` |
  | TCP | `crates/atm-daemon/src/local_tcp_transport.rs` | `HttpFrameReader` | `write_local_http_response` |

- Dispatcher: `crates/atm-daemon/src/runtime_health.rs`
- Cache: `crates/atm-daemon/src/runtime_status_cache.rs`
- `RuntimeMemberState`, `RuntimeStatusSnapshot`, and `HeartbeatActivity`
  already exist in `crates/atm-core/src/protocol.rs` and do not require
  semantic change
- `HEARTBEAT_PATH` (`/v1/atm/heartbeat`) already exists in
  `crates/atm-core/src/api.rs`

## Active Issues

Phase AJ does not directly close any open GH issue. It is forward-looking
observability groundwork that later tooling (status surfaces, doctor
extensions, ATM overlays) will consume.

| Bucket | Issues | Disposition |
|---|---|---|
| AD-addressed | (#421, #440) | Out of scope — owned by Phase AD line |
| Missing features | (#423, #448) | Out of scope — separate CLI surfaces |
| Release quality | (#435, #90) | Out of scope |
| Integration | (#461) | Out of scope — AJ adds state, not harness emitters |
| Bugs | — | None targeted |
| Backlog | (#19, #36, #64, #100) | Unchanged |

## Design Rules

These rules bind every AJ sprint:

- **Observational-only constraint.** It is expressly forbidden for any code
  to branch on the presence or absence of `session_id` or `pid`. The fields
  are recorded state, never behavior inputs. No `if session_id.is_some()`
  style behavioral forks outside the "do we update the cache" write path
  itself.
- **Non-overwrite rule.** When `session_id` is `None` in an incoming request
  the existing cached `session_id` is left untouched. Same for `pid`. Only
  `Some(...)` values mutate cache state. This rule applies identically on
  the local dispatch path (UDS + TCP) and the HTTP heartbeat path.
- **In-memory only.** No new SQLite column, table, or migration. The
  `team_roster` table already covers durable `member_kind`/`agent_type`;
  session state is ephemeral by design.
- **Wire compatibility.** All new wire fields use
  `#[serde(default, skip_serializing_if = "Option::is_none")]`. Older
  binaries must be able to round-trip newer payloads and vice versa.
- **Env var precedence.** CLI resolution reads `ATM_SESSION_ID` and
  `ATM_PID` from the process environment. Hooks are expected to set both;
  CLI users may leave them unset.
- **Trusted local observation.** CLI activity is recorded only when both
  `ATM_IDENTITY` and `ATM_TEAM` are present and parseable. Args-only activity
  produces no observation. If explicit identity/team differs from environment,
  normal command semantics continue but observation is silently suppressed; a
  concise `info!` event is permitted. Matching arguments and environment
  produce a normal observation. Delegated use is neither an error nor warning.
- **Single-mechanism.** The cache is the only authoritative runtime state
  surface. Because UDS and TCP share `ApiRouter`, a single `touch_member()`
  call site inside the dispatcher is the only write path for the CLI side —
  do not add per-transport touch sites in `request_worker.rs` or
  `local_tcp_transport.rs`.
- **No behavioral branching.** Restated: any conditional on
  `session_id`/`pid` presence that alters routing, retry, notification, or
  dispatch semantics is a defect.
- **Hard policy boundary.** Session, pid, heartbeat activity, and derived
  agent state are unproven best-effort telemetry. They are forbidden inputs to
  routing, nudge, notification, retry, admission, delivery, or policy logic.
  Only cache-merge and snapshot-projection code may inspect them. Any future
  exception requires a named requirement, ADR, boundary record, and test. The
  pre-existing heartbeat process-identity-conflict guard is unchanged and out
  of AJ scope.
- **Closed ingress set.** Runtime observation may be updated only by (1) the
  existing heartbeat endpoint, (2) successful CLI `send`/`read`/`ack` with
  trusted environment identity/team, or (3) graft through its existing
  environment-derived caller context. Roster reload, daemon recovery,
  transport adapters, peer delivery, nudge code, and all other paths must not
  synthesize or mutate observation.
- **Field-level provenance and no default overwrite.** Cache state records the
  source and timestamp of the last state change and the last session change
  independently. `None`, absent session, and default `Unknown` are no-ops for
  existing state/session data; local CLI/graft activity cannot replace a
  heartbeat-derived state. Roster output renders provenance only with a defined
  state/session value.
- **One reset path.** A new crate-private
  `reset_member_observation(team, member, reason)` is the only method allowed
  to set state to `Unknown` or clear a defined session. Normal heartbeat,
  CLI, and graft update methods cannot write either default. AJ adds no
  production reset caller; tests exercise the method directly.
- **State meaning.** `Unknown` means no trustworthy ingress has established a
  runtime state (or the explicit reset method was used). `Offline` means the
  heartbeat path explicitly received `SessionEnded`. They are never aliases:
  a normal update cannot convert either one into the other, and roster output
  must preserve the distinction.
- **Anomalies are not states.** AJ's roster lifecycle is only `Unknown`,
  `Active`, `Idle`, and `Offline`. Identity conflicts and malformed/suppressed
  observation are structured retained anomalies, not lifecycle values. AJ does
  not expose conflict as roster state or make a decision from it. A future
  doctor phase may diagnose retained anomaly events.
- **Known-state transitions.** A successful environment-attested CLI or graft
  `send`, `read`, or `ack` transitions the member to `Active`. Heartbeat maps
  explicit activity to `Active`, `Idle`, or `Offline` (`SessionEnded`). These
  known transitions update field provenance. Missing optional metadata is a
  no-op; it cannot manufacture `Unknown` or `Offline`.
- **Hook transition contract.** The external activity hooks use the existing
  heartbeat endpoint: startup/active → `ActiveToolUse`, idle → `Idle`, and
  stop → `SessionEnded` (`Offline`). AJ consumes these values only; hook-side
  installation and emission remain outside atm-core.

### Pid Overwrite Policy

`TeamMemberHeartbeatRequest.pid` is `u32` (required, always present), while
`WriteRequest.pid` and `ReadQuery.pid` are `Option<u32>`. This asymmetry is
intentional:

- **Heartbeat path: pid always overwrites.** A heartbeat is the canonical
  liveness authority for a roster member — the sending agent is asserting
  "this is the process that is alive right now." Every heartbeat therefore
  unconditionally replaces the cached `pid` with the request's `pid` value,
  including overwriting a pid previously set by a CLI dispatch.
- **CLI dispatch path: pid only sets when `Some`.** CLI-supplied pid is
  best-effort observational metadata from the calling process; a `None`
  must never erase state the heartbeat authority previously recorded.

This policy is an explicit, deliberate exception to the symmetric
non-overwrite rule above, which governs `session_id` on both paths and
`pid` on the CLI dispatch path only.

## Architecture Decisions

### AD-AJ-1: Session/pid state is in-memory only (no SQLite persistence)

**Decision.** `RuntimeStatusCache` entries for `session_id` and `pid` live
only in process memory. No new SQLite column, table, or migration is
introduced in Phase AJ.

**Rationale.**

- The state is ephemeral by design: a `session_id` identifies a live
  agent session and a `pid` identifies a live process. Both are
  meaningless after a daemon restart — persisting them would record
  stale liveness claims that the next daemon incarnation cannot trust.
- Avoiding the migration tax keeps Phase AJ scoped to observability
  groundwork; the `team_roster` table already covers the durable
  concerns (`member_kind`, `agent_type`).
- In-memory reads are lock-cheap and keep the dispatch hot path free of
  SQLite write contention.

**Reconsideration triggers.** This decision should be revisited if any of
the following become requirements:

- Post-restart forensics ("which session was active before the daemon
  died?") demanded by an operator surface.
- Session history/auditing across daemon lifetimes.
- Cross-host aggregation that outlives any single daemon process.

**Migration path if persistence is added later.** Add a
`runtime_session_state` table keyed by `(team, member)` with
`session_id TEXT NULL`, `pid INTEGER NULL`, `updated_at TEXT`, owned by
the daemon and written through the same `touch_member` /
`record_heartbeat` call sites. Because Phase AJ funnels all writes
through those two sites, persistence is an additive change confined to
`runtime_status_cache.rs` — no wire or protocol change is required, and
old in-memory-only daemons remain wire-compatible.

### AD-AJ-2: Pid overwrite asymmetry (heartbeat authoritative)

Captured under "Pid Overwrite Policy" in Design Rules above; recorded
here as a decision so future readers find it in both places.

## Scope Rules

Phase AJ may:

- add `SessionId` as a new core type
- extend `TeamMemberHeartbeatRequest` / `TeamMemberHeartbeatResponse`
- extend `WriteRequest` and `ReadQuery` with optional `session_id` and `pid`
- extend `CallerContext` and add env-var resolvers for `ATM_SESSION_ID` /
  `ATM_PID`
- extend `RuntimeStatusCache` entries with `session_id` and `pid`, and add
  the `touch_member` write path used by dispatch
- extend `RuntimeStatusSnapshot` to surface `session_id` per member
- extend the existing `atm members` roster projection to display observed
  `state` and `session_id` only when state is not `Unknown` or session is set
- extend the three CLI commands (`send`, `read`, `ack`) to populate the new
  fields from `CallerContext`
- add unit and integration tests proving the non-overwrite rule and the
  dual-path (UDS + TCP + HTTP heartbeat) update flow

Phase AJ must not:

- persist `session_id` or `pid` to SQLite
- introduce behavior changes triggered by `session_id`/`pid` values
- remove or rename any existing wire field, cache accessor, or heartbeat
  field
- add a new IPC channel, socket, HTTP route, or transport — both update
  paths reuse the existing unified HTTP-framed local transport and the
  existing heartbeat endpoint
- modify `local_ipc_transport/request_worker.rs` or `local_tcp_transport.rs`
  framing logic — the touch happens inside `runtime_health.rs` dispatch
- add a new operator surface or change routing/notification behavior from
  runtime observation
- change `RuntimeMemberState` semantics (Unknown, IdentityConflict, Offline,
  Idle, Active)

## Scope Deferred

- `atm doctor` / `atm status` surface changes that consume
  `RuntimeStatusSnapshot.session_id`
- Durable (SQLite) persistence of session history, if ever desired
- Any pid-liveness probing, reaping, or process-tree tracking
- Heartbeat TTL/expiry tuning beyond what already exists
- ATM overlay consumption of session state (alpha-prime / overlay work)
- Hook-side emitter changes (those land in their own repos)
- Surfacing pid in roster output or `RuntimeStatusSnapshot` (cache-internal)

## Execution Order

Strict merge-forward: `AJ.1 → AJ.2 → AJ.3 → AJ.4 → AJ.5 → AJ.6`, all on top
of `integrate/phase-AJ`.

| Sprint | Title | Purpose |
|---|---|---|
| AJ.1 | SessionId Type And Protocol Extensions | New `SessionId` type; heartbeat protocol structs gain `session_id` |
| AJ.2 | CallerContext Env Resolution | `ATM_SESSION_ID` / `ATM_PID` env resolvers |
| AJ.3 | CLI Wire Payload Integration | `send`/`read`/`ack` populate new fields; `WriteRequest`/`ReadQuery` extended |
| AJ.4 | Daemon Cache Touch On Dispatch | `touch_member` write path; non-overwrite rule on the unified dispatch path |
| AJ.5 | HTTP Heartbeat Session State | `record_heartbeat` accepts and stores `session_id` |
| AJ.6 | Snapshot Surface And Integration Validation | Snapshot exposure, integration tests, phase closeout |

No AJ pair is parallel-safe: every successor consumes its parent's public
protocol or cache contract. A child may start after its parent's development
commit is pushed; QA approval is not required. Merge parent → child before
every dev/fix round. A child PR cannot complete before its parent PR merges.

## Phase Exit Criteria

Phase AJ closes when all of the following hold:

- `SessionId` exists as a single canonical core type
- `TeamMemberHeartbeatRequest` / `TeamMemberHeartbeatResponse` carry
  `session_id` and round-trip on the wire
- `WriteRequest` and `ReadQuery` carry optional `session_id` and `pid`
  with wire-compatible serde attributes
- `CallerContext` resolves `ATM_SESSION_ID` and `ATM_PID` from env and all
  three CLI commands pass the values through
- `RuntimeStatusCache` stores `session_id` and `pid`, exposes
  `session_id` on `RuntimeStatusSnapshot`, and honors the non-overwrite
  rule on both the local dispatch path (UDS + TCP) and the HTTP heartbeat
  path
- `atm members` displays non-default observed state/session for a roster member
  and omits the default `Unknown` / absent-session observation
- A single `touch_member()` call site inside `runtime_health.rs` covers
  both UDS and TCP — verified by an integration test that exercises both
  transports against the same identity
- `cargo build --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`, and `cargo test --workspace` are green
- Integration tests prove: (a) Some-value updates cache, (b) None-value
  leaves cache untouched, (c) UDS, TCP, and HTTP heartbeat paths converge
  on the same cache entry, (d) no code branches behaviorally on
  presence/absence
- All six sprint docs are marked `complete` in their frontmatter
