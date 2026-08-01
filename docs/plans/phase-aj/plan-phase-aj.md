---
title: Phase AJ Plan
status: planned
branch: plan/phase-aj
worktree: ../atm-core-worktrees/plan/phase-aj
---

# Phase AJ Plan

## Phase-entry gate

Phase AJ's planning and review baseline is
`integrate/phase-ai-31-33 @ 150391ecdf2e003185bff7d78427cd21509a7981`, the
line that contains the unified HTTP local transports over UDS and TCP.
Plan review, acceptance review, and source references compare against that
branch's recorded SHA. Before AJ.1 implementation begins, Phase AI must merge
to `develop`; team-lead records the resulting `develop` SHA, creates
`integrate/phase-AJ` from it, and completes the reconciliation gate below.
`integrate/phase-AJ` is the AJ implementation target, not the planning
baseline.

**Planning-review rule.** Before Phase AI merges, a plan finding must compare
an AJ contract to the pinned AI baseline, not an unrelated `develop` snapshot.
After the Phase AI merge, a reconciliation finding is valid only when it names
both the pinned AI SHA and post-merge `develop` SHA and identifies a changed AJ
target. It is resolved before AJ.1 development, not ignored.

**Phase-AI reconciliation gate.** Phase AI's PR merges to `develop` before any
AJ implementation branch is created or any AJ dev/fix round begins. Team-lead
then records the post-merge `develop` SHA, creates `integrate/phase-AJ` at that
SHA, and diffs the pinned AI baseline against that SHA for every AJ exact target
(including AJ.3 `ack/mod.rs` and AJ.5 `api.rs`). Any drift updates the AJ plan
or implementation target and is revalidated before AJ.1 starts. All AJ
implementation branches inherit this recut target through their mandatory
parent → child merge-forward chain. An accidentally pre-created AJ branch must
first merge the recut target and re-run its sprint validation before any dev or
fix; no AJ PR may merge to `develop` before Phase AI's merge is complete.

## Goal

Maintain runtime observational state for every roster member in the daemon's
`RuntimeStatusCache`, fed by two independent update paths that converge on a
single in-memory entry per roster member:

1. **CLI/graft command path** — `atm send`, `atm read`, `atm ack`, and graft
   carry an optional `ActivityObservation` on the existing local wire payload.
   The DTO exists only when environment identity/team attest the caller; the
   daemon touches the cache after successful dispatch via `touch_member()` in
   `runtime_health.rs`.
2. **HTTP heartbeat path** — `POST /v1/atm/heartbeat` carries an optional
   `session_id`; the daemon records it through `record_heartbeat()` alongside
   the existing heartbeat activity.

State is in-memory only. No SQLite persistence. The fields are observational
only — recorded, never acted upon, and never retained with mail.

## Baseline

- **Planning baseline: `integrate/phase-ai-31-33 @
  150391ecdf2e003185bff7d78427cd21509a7981`**, recorded before plan review.
  It is the source of truth for the existing HTTP/UDS/TCP transport
  architecture and every AJ code-comparison finding.
- **Implementation target: `integrate/phase-AJ`**, created from the accepted
  post-Phase-AI-merge `develop` SHA after the reconciliation gate. All AJ work
  merges forward from this line.
- Research: `docs/plans/phase-aj/phase-aj-research.md`
- Governing contracts: `REQ-CORE-RUNTIME-002`, `REQ-CORE-RUNTIME-004`,
  `docs/adr/ADR-045-runtime-observation-attribution.md`, and
  `docs/team-member-state.md`. AJ.6 implements snapshot projection; AJ.7
  adds the source-use guard; AJ.8 records that guard at the daemon boundary;
  AJ.9 reconciles governing documents; and AJ.10 performs evidence-backed
  phase closeout.
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
  already exist in `crates/atm-core/src/protocol.rs`; AJ removes the
  `IdentityConflict` producer path and adds an additive member-observation
  projection to the snapshot
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
- **Latest accepted trusted observation wins.** "Latest" means accepted ingress
  order, not client-clock ordering. `None` leaves a cached optional value
  untouched. A trusted `Some(session_id)` replaces the current session; a
  heartbeat's required pid replaces the current pid and a trusted CLI/graft
  `Some(pid)` does the same. A distinct value is retained diagnostic evidence,
  not a rejection or lifecycle transition.
- **In-memory only.** No new SQLite column, table, or migration. The
  `team_roster` table already covers durable `member_kind`/`agent_type`;
  session state is ephemeral by design.
- **Wire compatibility.** All new wire fields use
  `#[serde(default, skip_serializing_if = "Option::is_none")]`. New readers
  accept older payloads with defaults; existing readers must accept and ignore
  the additive fields. AJ does not promise lossless re-serialization through
  an older binary that does not know those fields.
- **Env var precedence.** CLI resolution reads `ATM_SESSION_ID` and
  `ATM_PID` from the process environment as optional local metadata. The
  heartbeat's existing pid remains required; a hook may omit its optional
  session ID on `SessionEnded`, and CLI users may leave both environment values
  unset.
- **Trusted local observation.** CLI activity is recorded only when both
  `ATM_IDENTITY` and `ATM_TEAM` are present and parseable. Args-only activity
  produces no observation. If explicit identity/team differs from environment,
  normal command semantics continue but observation is silently suppressed; a
  concise `info!` event is permitted. Matching arguments and environment
  produce a normal observation. Delegated use is neither an error nor warning.
- **Transport trust boundary.** The daemon does not read its own environment
  or cryptographically prove the DTO's provenance. It accepts
  `ActivityObservation` only from existing authenticated local UDS/loopback
  ingress, where the client performed environment attestation, and clears it
  at remote HTTPS peer ingress before shared dispatch.
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
  exception requires a named requirement, ADR, boundary record, and test. AJ
  removes the existing live-pid conflict behavior: no heartbeat rejection,
  `IdentityConflict` cache write, readiness degradation, or cache-eviction
  special case may remain.
- **Closed ingress set.** Runtime observation may be updated only by (1) the
  existing heartbeat endpoint, (2) successful CLI `send`/`read`/`ack` with
  trusted environment identity/team, or (3) graft through its existing
  environment-derived caller context. Roster reload, daemon recovery,
  transport adapters, peer delivery, nudge code, and all other paths must not
  synthesize or mutate observation.
- **Field-level provenance and no default overwrite.** Cache state records the
  source and timestamp of the last state change and the last session change
  independently. `None`, absent session, and default `Unknown` are no-ops for
  existing state/session data. A trusted CLI/graft action sets `Active`; the
  next heartbeat may set its explicit state. Roster output renders provenance
  only with a defined state/session value.
- **State-change timestamp.** Each cache member has
  `state_changed_at: Option<IsoTimestamp>`. It is initialized only with a real
  known state and changes only when the lifecycle value changes; activity time
  may advance without rewriting it. The roster projection shows it only with a
  defined non-`Unknown` state, rendering a human relative age (for example,
  `Idle — 30m`) while retaining the absolute timestamp in structured output.
  Repeated evidence of the current state is not an edge and never resets this
  timestamp. Every lifecycle value follows the same rule: record the first
  entry into that value, then retain its timestamp until a different-state edge.
- **Change audit.** Every actual pid or session-ID mutation, including its
  initial set, emits one structured retained `info!` event with team, member,
  ingress source, timestamp, previous value, and new value. Raw values are
  allowed in this diagnostic event. No-op/missing/default input emits no
  mutation event. This audit event is diagnostic only.
- **No ordinary reset.** AJ adds no reset API. Removing a roster member drops
  its runtime entry; a later re-add starts `Unknown`. A roster metadata update
  preserves observation. Normal heartbeat, CLI, and graft updates cannot clear
  a known session or replace a known state with `Unknown`.
- **State meaning.** `Unknown` means no trustworthy ingress has established a
  runtime state. `Offline` means the
  heartbeat path explicitly received `SessionEnded`. They are never aliases:
  a normal update cannot convert either one into the other, and roster output
  must preserve the distinction.
- **Anomalies are not states.** AJ's roster lifecycle is only `Unknown`,
  `Active`, `Idle`, and `Offline`. A changed pid/session or suppressed
  observation emits retained structured evidence and then preserves the
  lifecycle transition dictated by its ingress. AJ does not expose conflict as
  roster state or make a decision from it. Existing `IdentityConflict` wire
  support may remain only for backward deserialization; AJ adds no producer.
  Doctor aggregation is future work; the retained log is sufficient in AJ.
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
`ActivityObservation.pid` on `WriteRequest` and `ReadQuery` is `Option<u32>`.
This asymmetry is
intentional:

- **Heartbeat path: pid always overwrites.** Heartbeat has a required pid, so
  every heartbeat replaces the cached current pid, including one previously
  supplied by CLI dispatch. This records its newest observation; it does not
  establish a liveness policy.
- **CLI dispatch path: pid only sets when `Some`.** CLI-supplied pid is
  best-effort observational metadata from the calling process; a `None`
  must never erase state the heartbeat authority previously recorded.

This policy is an explicit, deliberate exception to the `None`-preserves rule:
heartbeat has no absent pid value, while local CLI/graft ingress does.

## Architecture Decisions

### AD-AJ-1: Session/pid state is in-memory only (no SQLite persistence)

**Decision.** `RuntimeStatusCache` entries for `session_id` and `pid` live
only in process memory. No new SQLite column, table, or migration is
introduced in Phase AJ.

**Rationale.**

- The state is ephemeral: it is the daemon's current trusted observation, not
  a durable liveness claim. A retained structured transition event supplies AJ
  diagnostics without writing telemetry on every mail operation.
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

**Future persistence decision.** A later requirement and ADR must choose the
history model, retention, privacy, recovery semantics, and write budget before
adding any durable table. It may reuse AJ's two ingress adapters, but Phase AJ
does not pre-approve a schema or imply that the current observation should
survive a daemon restart.

### AD-AJ-2: Pid overwrite asymmetry (heartbeat required field)

Captured under "Pid Overwrite Policy" in Design Rules above; recorded
here as a decision so future readers find it in both places.

### AD-AJ-3: Rust type and error shape

- **RBP-004 Newtype.** `SessionId` is one transparent core newtype. PID stays
  the existing `u32` protocol field: AJ adds no new semantic invariant beyond
  rejecting an invalid optional local environment value, so a second wrapper
  would add conversion churn without preventing a real class of error.
- **RBP-002 Typestate.** Runtime observation is a dynamic, shared cache fed by
  independent external events. It is intentionally one closed merge function,
  not a typestate API; the values must coexist in snapshots and can legally
  transition among `Active`, `Idle`, and `Offline` in accepted-ingress order.
- **RBP-001 / RBP-007.** AJ adds no user-facing failure path for observation.
  The cache merge is infallible and cannot turn a successful command or
  heartbeat into an error. Existing command and heartbeat errors retain their
  current contracts; malformed or suppressed optional telemetry is an
  informational diagnostic, not a new error result.

## Scope Rules

Phase AJ may:

- add `SessionId` as a new core type
- extend `TeamMemberHeartbeatRequest` / `TeamMemberHeartbeatResponse`
- add transient `ActivityObservation` and carry it optionally on `WriteRequest`
  and `ReadQuery`, preserving it through the `AckRequest` conversion into the
  canonical `WriteRequest`
- extend `CallerContext` and add env-var resolvers that construct an optional
  environment-attested `ActivityObservation`
- extend `RuntimeStatusCache` entries with `session_id` and `pid`, and add
  the `touch_member` write path used by dispatch
- extend `RuntimeStatusSnapshot` and `atm members` to surface current
  state/session/pid/provenance only when defined; JSON uses raw values and
  human output shortens the session identifier
- extend CLI and graft read/send/ack callers to populate the optional
  observation from their environment-derived context
- add unit, integration, and narrow boundary tests proving latest-observation
  merge semantics, the dual-path (UDS + TCP + HTTP heartbeat) update flow,
  and that observation cannot enter policy code

Phase AJ must not:

- persist `session_id` or `pid` to SQLite, mail rows, or mail payloads
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
- produce or act on `RuntimeMemberState::IdentityConflict`; compatibility-only
  deserialization is permitted

## Scope Deferred

- `atm doctor` / `atm status` surface changes that consume
  `RuntimeStatusSnapshot.session_id`
- Durable (SQLite) persistence of session history, if ever desired
- Any pid-liveness probing, reaping, or process-tree tracking
- Heartbeat TTL/expiry tuning beyond what already exists
- ATM overlay consumption of session state (alpha-prime / overlay work)
- Hook-side emitter changes (those land in their own repos)
- Durable session/PID span history or post-restart session recovery

## Execution Order

Strict merge-forward: `AJ.1 → AJ.2 → AJ.3 → AJ.4 → AJ.5 → AJ.6 → AJ.7 → AJ.8 → AJ.9 → AJ.10`, all on top
of `integrate/phase-AJ`.

AJ.1 starts only after the Phase-AI reconciliation gate above. That gate is a
phase-entry dependency, not parent-sprint QA: once it passes, AJ.1 and every
successor follow the immediate merge-forward rule below without waiting for
their parent QA outcome.

| Sprint | Title | Purpose |
|---|---|---|
| AJ.1 | SessionId Type And Protocol Extensions | New `SessionId` type; heartbeat protocol structs gain `session_id` |
| AJ.2 | CallerContext Env Resolution | `ATM_SESSION_ID` / `ATM_PID` env resolvers |
| AJ.3 | CLI Wire Payload Integration | `send`/`read`/`ack` populate new fields; `WriteRequest`/`ReadQuery` extended |
| AJ.4 | Daemon Cache Touch On Dispatch | `touch_member` write path; non-overwrite rule on the unified dispatch path |
| AJ.5 | HTTP Heartbeat Session State | `record_heartbeat` accepts and stores `session_id` |
| AJ.6 | Runtime Observation Snapshot Projection | Snapshot exposure and roster projection |
| AJ.7 | Runtime Observation Source-Use Guard | Narrow static guard for non-authoritative observation use |
| AJ.8 | Runtime Observation Boundary Record | Machine and human daemon boundary record |
| AJ.9 | Runtime Observation Governing Contract Reconciliation | Requirements, ADR, architecture, and team-state reconciliation |
| AJ.10 | Runtime Observation Phase Closeout | Evidence validation and phase/sprint/project closeout |

No AJ pair is parallel-safe. The immediate successor starts as soon as its
parent development head is available: merge parent → child, then begin child
development. Parent QA approval is never a gate for child development. Before
every child dev or fix round, merge the current parent head into the child
branch. A child PR cannot complete or merge its target before its parent PR
merges. Here, “parent merged” for a dev/fix round means this mandatory
parent-branch → child-branch merge-forward, not parent PR completion.

| Parent | Immediate successor | Merge-forward reason |
|---|---|---|
| AJ.1 | AJ.2 | `SessionId` protocol contract |
| AJ.2 | AJ.3 | environment-attested observation DTO |
| AJ.3 | AJ.4 | local-wire request fields |
| AJ.4 | AJ.5 | sole cache-merge contract |
| AJ.5 | AJ.6 | converged heartbeat/cache state |
| AJ.6 | AJ.7 | implemented snapshot/roster targets |
| AJ.7 | AJ.8 | passing source-use enforcement gate |
| AJ.8 | AJ.9 | final boundary record |
| AJ.9 | AJ.10 | reconciled governing contracts |

## Phase Exit Criteria

Phase AJ closes when all of the following hold:

- [ ] `SessionId` exists as a single canonical core type
- [ ] Phase AI merged to `develop`; the post-merge SHA and AJ planning-baseline
  SHA were recorded, AJ target exact paths were reconciled, and
  `integrate/phase-AJ` was cut from that post-merge SHA before AJ.1 began
- [ ] `TeamMemberHeartbeatRequest` / `TeamMemberHeartbeatResponse` carry
  `session_id` and round-trip on the wire
- [ ] `ActivityObservation` is one optional, wire-compatible DTO on `WriteRequest`
  and `ReadQuery`; it carries team/member plus optional session/pid only for
  environment-attested local callers, and HTTPS peer ingress clears it before
  shared dispatch
- [ ] `CallerContext` resolves `ATM_SESSION_ID` and `ATM_PID` from env; CLI and
  graft read/send/ack pass the optional observation through only when the
  environment attests the resolved caller, including the `AckRequest` →
  canonical `WriteRequest` conversion
- [ ] `RuntimeStatusCache` stores the current `session_id` and `pid`, exposes both
  on `RuntimeStatusSnapshot`, and applies latest-accepted-trusted-observation
  semantics on local dispatch (UDS + TCP) and HTTP heartbeat paths
- [ ] `atm members` displays defined observed state age, pid, and shortened session
  for a roster member; it omits default `Unknown` / absent-session observation
- [ ] A single `touch_member()` call site inside `runtime_health.rs` covers
  both UDS and TCP — verified by an integration test that exercises both
  transports against the same identity; it runs only after a successful local
  write/read, never after a failed dispatch or through remote ingress
- [ ] `cargo build --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`, and `cargo test --workspace` are green
- [ ] Integration tests prove: (a) trusted latest values update cache and `None`
  leaves it untouched, (b) UDS, TCP, and HTTP heartbeat paths converge on the
  same cache entry, (c) a changed pid/session is retained evidence only, and
  (d) no code branches behaviorally on observation state
- [ ] The AJ.7 source-use guard has required-positive checks and rejects policy
  consumers of runtime observation
- [ ] AJ.8 daemon boundary records agree with the merged implementation
- [ ] AJ.9 requirements, ADR, architecture, and team-member-state agree with
  the merged implementation
- [ ] All ten sprint docs are marked `complete` in their frontmatter
