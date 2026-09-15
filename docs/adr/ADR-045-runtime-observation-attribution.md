# ADR-045 — Runtime Observation Attribution

| Field | Value |
| --- | --- |
| Status | Accepted — amended by issue #1378 and Phase BA |
| Scope | Phase AJ runtime observation; canonical roster-state amendment |
| Relates to | `REQ-CORE-RUNTIME-002`, `REQ-CORE-RUNTIME-004`, ADR-014, ADR-015, ADR-062, issue #1378 |

## Original Phase AJ decision

The following records the implemented Phase AJ baseline. The issue #1378
amendment below supersedes its owner and accepted state-ingress set, and the
Phase BA task-reminder exception narrows its blanket ban on state-based
attention; all other attribution and trust-boundary clauses remain active.

Session, pid, heartbeat activity, and derived agent state are in-memory,
best-effort telemetry. They are forbidden inputs to routing, nudge,
notification, retry, admission, delivery, and policy decisions because the
state is neither complete nor proven current.

A successful local command updates telemetry only if `ATM_IDENTITY` and
`ATM_TEAM` are present and agree with any CLI identity/team arguments.
Args-only or mismatched commands retain normal behavior but suppress telemetry;
an info-level diagnostic is allowed. The existing heartbeat ingress remains a
separate telemetry path. Graft may use only its environment-derived caller
context. Roster reload, recovery, transport adapters, peer delivery, and nudge
paths are not telemetry ingress.

Local read/write DTOs carry one optional `ActivityObservation` containing the
attested team/member and optional session/pid. It is transient, never mail
data. The daemon accepts it only over the existing authenticated local
UDS/loopback ingress; it does not read its own environment or prove the DTO's
provenance. Remote HTTPS ingress clears it before shared dispatch.

State and session retain separate last-change source/timestamp provenance.
Absent/default data is a no-op and cannot overwrite a defined observation.
Accepted-ingress order, not client-clock order, determines the current value. A
trusted changed pid/session becomes the current observation and emits retained
diagnostic evidence. Normal heartbeat, CLI, and graft ingestion cannot restore
defaults; roster removal drops its runtime entry and a later re-add starts
without observation.

Every actual pid/session mutation emits one structured diagnostic audit event
with prior/new value, member, source, and timestamp. No-op input emits none.
The existing heartbeat `pid_changed` response field remains true only for
replacement of a prior defined pid; an initial PID is audited but is not a
replacement.

`Unknown` means no trustworthy state observation; `Offline` requires an
explicit heartbeat session-end event. They are never interchangeable.

Successful environment-attested CLI/graft send, read, and ack are `Active`;
heartbeat maps its explicit activity to `Active`, `Idle`, or `Offline`.
`state_changed_at` records only a real lifecycle transition, not a metadata or
same-state activity update.
External hooks emit startup/active, idle, and stop through that existing
heartbeat contract; hook-side implementation is outside this repository.

Identity change and malformed/suppressed observation are retained anomaly
events, not lifecycle states. They never reject ingress, emit
`IdentityConflict`, degrade readiness, alter cache eviction, or change
routing/nudge/delivery behavior. A future doctor phase may diagnose them.

An exception requires an explicit requirement, ADR, boundary record, and test.

The existing roster view may render defined state age, pid, and a shortened
session for its matching member. JSON retains raw values; human output omits
default `Unknown` / absent-session telemetry and never uses display state to
make a workflow decision.

## Issue #1378 amendment

The original default-deny policy remains, with one explicit exception and a
corrected owner:

1. The write-through RAM master roster owns exactly one ephemeral
   `RuntimeMemberState` per durable `(team, member)`. `RuntimeHealth` is a
   projection-only reader under ADR-014; it must not retain or merge a second
   member-state map.
2. Authenticated local heartbeat POSTs and successful Herdr list polls converge
   on that same record. Source and timestamps describe the latest accepted
   observation; they do not create source-specific states. Accepted ingress
   order remains authoritative. The pre-cutover `ActivityObservation` request
   field remains tolerated for wire compatibility but is not canonical state
   ingress and cannot produce a runtime observation source.
3. Each accepted state observation advances a typed `RosterStateRevision` and
   `last_observed_at`, even for same-state evidence. `state_changed_at` still
   changes only on a state edge. A failed/incomplete poll makes no state or
   revision write; it may update typed availability/attempt metadata.
4. A successful Herdr poll maps working to `Active`, idle/done to `Idle`,
   blocked to `Blocked`, and covered unknown/absent members to `Unknown`.
   `Offline` remains exclusive to an explicit heartbeat `SessionEnded` event;
   no failed poll, timeout, absent snapshot, or projection gap may synthesize
   `Offline` or `Dead`.
5. Task-reminder eligibility is the sole policy exception (ADR-062, Phase BA
   amendment). The reminder path reads the exact canonical `RuntimeMemberState`
   from the master roster: `Idle` is eligible, `Active` is never interrupted,
   `Blocked` and `Offline` receive no reminder and are escalated. It must not
   consume `PickerMemberStatus`, a `RuntimeHealth` projection, or raw Herdr
   list results as its eligibility authority. Poll/heartbeat ingress never
   reads queues/tasks or emits directly; delivery-channel policy remains
   downstream.
6. Session, pid, source, and freshness metadata remain forbidden policy inputs.
   Any additional state consumer still requires a requirement, ADR, boundary
   record, and regression test.

This amendment deliberately removes the older claim that every derived state
is telemetry-only. It does not authorize state-based routing or delivery, and
it does not modify the frozen synchronous daemon.

## Phase AJ baseline implementation evidence

This table identifies the pre-#1378 implementation that the amendment replaces
where it names `RuntimeStatusCache` as the member-state owner. The issue #1378
fix must replace those anchors with master-roster mutation/projection evidence;
the table remains as historical traceability for the retained AJ clauses.

| Clause | Source symbol | Test evidence |
| --- | --- | --- |
| Accepted local/heartbeat ingress and no-overwrite merge | `RuntimeStatusCache::merge_observation` | `normal_updates_never_regress_known_state_or_session_to_default` |
| Remote stripping and local-only cache touch | `clear_remote_activity_observation`, `TrustedActivityObservation::from_local` | `peer_wire_strips_forged_activity_observation_before_routing` |
| No conflict policy; changed metadata retained | `RuntimeStatusCache::record_heartbeat` | `changed_pid_or_session_is_retained_evidence_not_identity_conflict` |
| Snapshot projection and roster scoping | `build_runtime_snapshot_scoped`, `RuntimeStatusCache::snapshot_for_members` | `runtime_status_cache_scoped_snapshot_reads_do_not_require_shared_locking`, `run_lists_member_roster_without_daemon` |
| Human/JSON roster projection | `MembersCommand::print_members_result`, `render_runtime_observation` | `short_session_id_uses_unicode_scalar_limit`, `run_lists_member_roster_without_daemon` |
| Cross-transport convergence | `DaemonRequestDispatcher::dispatch` | `heartbeat_and_local_dispatch_converge_on_cache`, `send_read_ack_reflects_each_caller_session_in_runtime_cache` |
| Session provenance edge gating | `RuntimeStatusCache::merge_observation` | `session_changed_by_and_at_update_only_on_session_edge` |
| PID replacement semantics | `ObservationMergeOutcome::pid_changed` | `pid_changed_response_is_false_for_initial_set_and_true_for_replacement` |
| Lifecycle state mapping | `RuntimeStatusCache::record_heartbeat` | `unknown_and_offline_are_distinct_states_with_distinct_provenance`, `trusted_cli_activity_transitions_offline_or_idle_to_active` |
| State-edge timestamps | `RuntimeStatusCache::merge_observation` | `state_changed_at_updates_only_on_real_state_transition`, `state_changed_by_updates_only_on_real_state_transition` |
| Audit event emission | `RuntimeStatusCache::emit_metadata_change` | `session_and_pid_mutations_emit_exactly_one_audit_event`, `no_op_metadata_updates_emit_no_audit_event` |
| Heartbeat activity ingress | `RuntimeStatusCache::record_heartbeat` | `heartbeat_session_id_round_trip` |
| External-hook activity mapping | `HeartbeatActivity` conversion at heartbeat ingress | `heartbeat_session_id_round_trip` |
| Identity/anomaly handling | `RuntimeStatusCache::merge_observation` | `pid_conflict_replacement_emits_one_audit_event_without_identity_conflict` |
| Pre-AJ reader compatibility | `RuntimeStatusSnapshot` serde defaults | `runtime_status_snapshot_accepts_pre_aj_payload_without_members` |
| Older reader additive-field compatibility | `RuntimeStatusSnapshot::members` additive field | `older_runtime_snapshot_reader_ignores_additive_members_field` |
| Default-deny source use | `.just/check_runtime_observation_boundary.py` | `RuntimeObservationBoundaryTests` |
