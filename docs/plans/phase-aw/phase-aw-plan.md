---
title: "Phase AW — Unified retained runtime logging, graft observability, and native tool parity"
phase: AW
branch: plan/phase-aw
sprint_branches: per-sprint, declared in each sprint doc's frontmatter; all
  four form one `gh stack` rooted at integrate/phase-aw (AW.0 provisioning
  task, owner team-lead).
status: draft — awaiting quality-mgr plan review
owner: fenix (plan author, coordinator); dev agents per sprint
base_revision: ba4c91bb3 (develop)
integration_branch: integrate/phase-aw
issues: "#905 (unify retained runtime logging), #904 (graft observability bindings), #952 (hermes-atm native tool parity with atm CLI)"
dependency_relations:
  - prerequisite: AW.1
    dependent: AW.2
    relation: must_follow
  - prerequisite: AW.2
    dependent: AW.3
    relation: must_follow
  - related: AW.4
    relation: parallel_safe
    scope: parallel_safe with AW.1 and AW.2 (graft-python + atm-observability
      path constants only); AW.4's merged-view acceptance is verified by AW.3
  - prerequisite: AW.4
    dependent: AW.3
    relation: must_follow
    scope: AW.3's merged `atm log` view must read the AW.4 satellite file
  - prerequisite: AW.4
    dependent: AW.5
    relation: must_follow
    scope: same projection surface (atm-graft-python result types,
      hermes-atm native_tools.py); AW.4 lands the envelope shape first
  - related: AW.5
    relation: parallel_safe
    scope: parallel_safe with AW.3 (disjoint files)
---

# Phase AW — Unified retained runtime logging and graft observability

> Evidence base: read-only code map of develop @ ba4c91bb3 (2026-09-02),
> GitHub issues #905 and #904 (Rand, 2026-08-16) and #952 (Rand,
> 2026-08-19). #905 and #904 are delivered together per #905's own
> requirement; #952 was added 2026-09-03 (team-lead) because it edits the
> same hermes-atm / atm-graft-python projection surface as AW.4.

## 1. Problem

The documented retained-log contract
([`docs/atm-daemon/logging.md`](../../atm-daemon/logging.md) §"default
retained event set", ADR-011) promises that every ATM `warn!` and `error!`
event is retained in `atm.log.jsonl`. On develop today that promise is not
met on the Tokio/Axum daemon path, and native graft callers have no retained
path at all:

1. **No tracing bridge.** `DaemonObservability`
   (`crates/atm-daemon-bootstrap/src/daemon_observability.rs`) wraps an
   `sc_observability::Logger`, but no `tracing` subscriber is installed in
   the daemon process. The only subscriber in the workspace is the CLI's
   optional stderr `tracing_subscriber::fmt()` behind `--stderr-logs`
   (`crates/atm/src/main.rs:342-359`). Every `tracing::warn!`/`error!` in
   `atm-daemon-bootstrap`, `atm-http-runtime`, `atm-runtime` and
   `atm-storage-rusqlite` (51 call sites) is discarded in the daemon.
2. **Runtime stderr bypass.** Two post-bootstrap `eprintln!` sites remain on
   the live path (`crates/atm-daemon-bootstrap/src/lib.rs:520,524`,
   shutdown-signal and unexpected-server-stop notices).
3. **SQLite diagnostics discarded.** `SqliteObservability`
   (`crates/atm-storage-rusqlite/src/observability.rs`) has exactly one
   implementor, `NullSqliteObservability`, and every production construction
   site passes it (`lib.rs:741,750`, `shared_db_reader_lanes.rs:43`).
   Writer/WAL timeouts and failures vanish; `emit_or_warn`'s fallback
   `tracing::warn!` is itself lost per item 1.
4. **No SQLite diagnostic timeline.** `DB_MIGRATIONS` has no diagnostic
   event table.
5. **Silent overload.** `Logger::try_log` returns `QueueFull` under
   saturation; the daemon maps it to `Ok(())`
   (`daemon_observability.rs:317`). `queue_full_drops_total` is exposed in
   `atm doctor` observability detail but no retained diagnostic marks the
   transition.
6. **Graft blind spot (#904).** `atm-graft-python` classifies
   daemon-unavailable failures and performs `reconnect_client()` recovery
   (`crates/atm-graft-python/src/lib.rs:433-644`) but imports no
   observability crate; nothing is retained for native Hermes calls.
   `atm log` (`crates/atm/src/commands/log.rs`) reads one JSONL file only.
7. **Native tool parity gap (#952).** `crates/hermes-atm/src/hermes_atm/native_tools.py`
   projects `atm_list/atm_read/atm_send` results by hand
   (`_list_result/_read_result/_send_result`) and the binding's result
   types (`crates/atm-graft-python/src/tool_types.rs`) carry no envelope:
   `from_agent` vs CLI `from`, no `bucket_counts`/`selection_mode`/
   `history_collapsed`/`action`/`team`/`agent`/`sender`/`summary`,
   `+00:00` vs `Z` timestamps, and `atm_read` never returns the body the
   store holds.

## 2. Acceptance contract (binding for the phase)

The two issue checklists are the literal deliverable list. Each sprint doc
maps its acceptance criteria back to the checklist items it closes; the phase
is complete only when every checkbox in #905, #904, and #952 is claimed by
exactly one sprint and verified by quality-mgr.

Phase-wide invariants (apply to every sprint):

- **Tokio/Axum only.** No change to the frozen synchronous daemon
  (`crates/atm-daemon` runtime/dispatch). AGENTS.md hard rule.
- **Never alter business outcomes.** A failure anywhere in the logging
  pipeline (bridge, JSONL sink, SQLite timeline, fallback satellite) MUST NOT
  change the result of a send, read, ack, delivery, or database write. Every
  sprint carries a non-interference test.
- **No recursion.** A diagnostic produced while handling a diagnostic
  (writer failure → diagnostic → writer failure …) must terminate. Every
  sink has a reentrancy guard and origin tagging.
- **Redaction allowlist, not denylist.** Retained records carry only
  allowlisted structured fields: timestamp, level, code, component/target,
  action/event, correlation id, outcome, elapsed, endpoint kind, bounded safe
  detail. Never message bodies, template values, recipients, chat IDs,
  credentials, tokens, raw env/config values, absolute user paths.
- **Bounded everywhere.** Queues, batch sizes, row counts, detail length, and
  file sizes are explicit constants with tests at the boundary.
- **Honest loss semantics.** Documentation and `atm log` output never claim
  lossless equivalence between JSONL and SQLite under overload.
- **sc-observability stays pinned** at `=1.2.0` unless a sprint records a
  concrete API gap; no fork or vendoring.

## 3. Sprints

| Sprint | Doc | Scope |
|---|---|---|
| AW.1 | [sprint-AW.1-tracing-bridge.md](./sprint-AW.1-tracing-bridge.md) | Process-wide `tracing` → sc-observability bridge layer for the daemon, structured field preservation, reentrancy guard, retirement of live-runtime `eprintln!`, documented pre-bootstrap stderr exceptions, shared observability path constants (canonical + fallback satellite). |
| AW.2 | [sprint-AW.2-sqlite-diagnostic-timeline.md](./sprint-AW.2-sqlite-diagnostic-timeline.md) | `diagnostic_events` table, bounded async timeline writer with retention/pruning, `DiagnosticTimelineStore` contract, production `SqliteObservability` adapter replacing `NullSqliteObservability`, saturation/transition diagnostics, drop counters. |
| AW.3 | [sprint-AW.3-health-and-log-query.md](./sprint-AW.3-health-and-log-query.md) | Drop/degradation counters in health and doctor output, `atm log --source jsonl|timeline|merged` with timestamp-ordered merge over canonical JSONL + graft fallback satellite + SQLite timeline, retained-log contract documentation rewrite. |
| AW.4 | [sprint-AW.4-graft-fallback-observability.md](./sprint-AW.4-graft-fallback-observability.md) | Rust-owned observability API on the `atm-graft-python` Maturin binding: path diagnostics, fallback satellite emitter, the four-event contract on daemon-unavailable/recovery paths, envelope diagnostic on fallback write failure, Python 3.11–3.14 tests. |
| AW.5 | [sprint-AW.5-native-tool-parity.md](./sprint-AW.5-native-tool-parity.md) | #952: typed list/read/send envelopes in `atm-core` shared by CLI `--json` and the binding; `atm_read` returns the full message; hermes-atm projections become pass-throughs; key-for-key parity test against the CLI. |

Dependency rationale: AW.2's production adapter emits through AW.1's bridge
(must_follow). AW.3 queries what AW.2 persists and merges what AW.4 writes
(must_follow both). AW.4 depends on AW.1 only for the shared path constant
in `atm-observability`, which AW.1 lands first as a one-line deliverable;
AW.4 is otherwise parallel_safe with AW.1 and AW.2. AW.5 must follow AW.4
(same projection files) and is parallel_safe with AW.3.

## 4. Execution notes

- Branch strategy: `gh stack` rooted at `integrate/phase-aw`, order
  AW.1 → AW.2 → AW.4 → AW.3 → AW.5 (AW.5 may branch from AW.4 directly
  and run alongside AW.3). Provisioning is task **AW.0** (team-lead):
  create `integrate/phase-aw` from `develop`, create the five sprint
  branches, run `gh stack init`, record `gh stack view --json` here.
- Each sprint: dev agent implements on its branch, PR opened on first push
  targeting the parent branch in the stack, `qa-pr<N>-r1` dispatched to
  quality-mgr immediately, CI is a merge gate only.
- Merge-forward on every parent merge (`feedback_merge_forward_asap`).
- Hermes-side (`hermes-atm`, out of repo) adopts the AW.4 binding API; no
  Hermes code lives here. Cross-team ask framed as a binding addition.
- Storage architecture rule: AW.2 must not add a second SQLite writer
  connection; diagnostic writes flow through the existing single writer lane
  as a low-priority, non-blocking batch op.

## 5. Out of scope (phase non-goals, from both issues)

- Persisting business messages or high-volume info telemetry in SQLite.
- Replacing JSONL with SQLite.
- New socket/queue/spool services; Hermes-specific log roots or files.
- Automatic replay of failed graft sends.
- Any legacy synchronous daemon change.

## 6. QA history

_(appended by the coordinator per quality-mgr round; plan is not
dispatchable until a PASS is recorded here.)_
