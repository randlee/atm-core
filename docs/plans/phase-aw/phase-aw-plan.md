---
title: "Phase AW — Unified retained runtime logging, graft observability, and native tool parity"
phase: AW
branch: plan/phase-aw
sprint_branches:
  - feature/aw1-tracing-bridge
  - feature/aw2-sqlite-diagnostic-timeline
  - feature/aw3-health-and-log-query
  - feature/aw4-graft-fallback-observability
  - feature/aw5-native-tool-parity
status: plan review PASS (round 4, 2026-09-03, see §6) — execution held until after the next release per Rand
owner: fenix (plan author, coordinator); dev agents per sprint
base_revision: ba4c91bb3 (develop)
integration_branch: integrate/phase-aw
issues: "#905 (unify retained runtime logging), #904 (graft observability bindings), #952 (hermes-atm native tool parity with atm CLI)"
branch_model: >
  Every sprint branch is cut from integrate/phase-aw and its PR targets
  integrate/phase-aw (standard phase flow, no gh stack). "must_follow" means
  the prerequisite sprint's PR is MERGED into integrate/phase-aw before the
  dependent sprint's branch is cut and dispatched. "parallel_safe" means the
  two sprints may be in flight at the same time because they touch disjoint
  files.
dependency_relations:
  - prerequisite: AW.1
    dependent: AW.2
    relation: must_follow
    scope: AW.2 wires its DiagnosticSink hook and adapter into AW.1's bridge layer
  - prerequisite: AW.1
    dependent: AW.4
    relation: must_follow
    scope: AW.4 consumes AW.1's graft_fallback_log_path constant and the
      atm-observability boundary record AW.1 creates
  - prerequisite: AW.2
    dependent: AW.3
    relation: must_follow
    scope: AW.3 queries the timeline AW.2 persists
  - prerequisite: AW.4
    dependent: AW.3
    relation: must_follow
    scope: AW.3's merged `atm log` view reads the AW.4 satellite file
  - prerequisite: AW.4
    dependent: AW.5
    relation: must_follow
    scope: same projection surface (atm-graft-python result types,
      hermes-atm native_tools.py); AW.4 lands the envelope `observability`
      field first
  - pair: [AW.2, AW.4]
    relation: parallel_safe
    scope: AW.2 edits atm-storage/atm-storage-rusqlite/atm-daemon-bootstrap;
      AW.4 edits atm-graft-python/hermes-atm/boundaries/atm-graft-python
  - pair: [AW.3, AW.5]
    relation: parallel_safe
    scope: AW.3 edits atm/commands/log.rs, atm-http-runtime, atm-core/doctor,
      docs; AW.5 edits atm-graft-python/tool_types.rs, hermes-atm, atm-core
      envelope structs (AW.3 does not touch list/read/send outcomes)
---

# Phase AW — Unified retained runtime logging, graft observability, and native tool parity

> Evidence base: read-only code map of develop @ ba4c91bb3 (2026-09-02),
> GitHub issues #905 and #904 (Rand, 2026-08-16) and #952 (Rand,
> 2026-08-19). #905 and #904 are delivered together per #905's own
> requirement; #952 was added 2026-09-03 (team-lead) because it edits the
> same hermes-atm / atm-graft-python projection surface as AW.4.
> Appendix A quotes every issue checklist item verbatim so coverage can be
> verified without GitHub access.

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
   (`crates/atm/src/main.rs:342-359`). Every non-test `tracing::warn!` /
   `tracing::error!` call site in `atm-daemon-bootstrap`, `atm-http-runtime`,
   `atm-runtime` and `atm-storage-rusqlite` is discarded in the daemon.
   Count on the base revision:
   `grep -rnE 'tracing::(warn|error)!|^\s*(warn|error)!\(' crates/{atm-daemon-bootstrap,atm-http-runtime,atm-runtime,atm-storage-rusqlite}/src` → 41 sites
   (the number is context, not an acceptance target; AW.1 AC1 tests one
   induced event per crate).
2. **Runtime stderr bypass.** Two post-bootstrap `eprintln!` sites remain on
   the live path (`crates/atm-daemon-bootstrap/src/lib.rs:520,524`,
   shutdown-signal and unexpected-server-stop notices).
3. **SQLite diagnostics discarded.** `SqliteObservability`
   (`crates/atm-storage-rusqlite/src/observability.rs`) has exactly one
   implementer, `NullSqliteObservability`, and both production construction
   sites pass it (`lib.rs:741,750`; the `shared_db_reader_lanes.rs`
   `open_in_memory_for_test` helper also does, but it is `#[cfg(test)]` and
   out of scope).
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
   (`crates/atm-graft-python/src/lib.rs:433-644`, policies
   `DaemonRecoveryPolicy::{RefreshOnly, RetryOnce}`) but imports no
   observability crate; nothing is retained for native Hermes calls.
   `atm log` (`crates/atm/src/commands/log.rs`) reads one JSONL file only.
7. **Native tool parity gap (#952).** The CLI's `--json` output serialises
   the canonical `atm-core` outcome structs (`ListOutcome`, `ReadOutcome`,
   `SendOutcome`: `action`, `team`, `agent`, `sender`, `summary`,
   `selection_mode`, `history_collapsed`, `bucket_counts`, …). The binding's
   result types (`crates/atm-graft-python/src/tool_types.rs`:
   `AtmSendResult`, `AtmReadResult`, `AtmListRow`, `AtmListResult`) carry a
   hand-picked subset, and `crates/hermes-atm/src/hermes_atm/native_tools.py`
   (`_send_result/_read_result/_list_result`) projects that subset again.
   Concretely: list rows use `from_agent` instead of `from`; no
   `bucket_counts`/`selection_mode`/`history_collapsed`/`action`/`team`/
   `agent`/`sender`/`summary`; timestamps serialise as `+00:00` instead of
   `Z`. The message body IS returned when a message is selected
   (`PyMessage::from_read` → `_message_result` `body`); the read-side gap is
   the envelope (`bucket_counts`) and message metadata (`summary`,
   `timestamp`, `requires_ack`, `task_id`, `chat_id`).

## 2. Acceptance contract (binding for the phase)

The issue checklists (Appendix A) are the literal deliverable list. Each
sprint doc maps its acceptance criteria to checklist ids; the phase is
complete only when every id in Appendix A is claimed by at least one
sprint and every facet of the id is verified by quality-mgr. A composite id
(one whose text names several behaviours, e.g. 905-6, 905-8, 904-4, 904-7)
may be claimed by several sprints; Appendix A lists every claiming sprint
with the facet it owns, each claiming sprint's ACs name that facet, and the
id is closed only when all listed facets are verified. No id may be
unclaimed.

Phase-wide invariants (apply to every sprint):

- **Tokio/Axum only.** No change to the frozen synchronous daemon
  (`crates/atm-daemon` runtime/dispatch). AGENTS.md hard rule.
- **Never alter business outcomes.** A failure anywhere in the logging
  pipeline (bridge, JSONL sink, SQLite timeline, fallback satellite) MUST NOT
  change the result of a send, read, ack, delivery, or database write. Every
  sprint carries a non-interference test.
- **No recursion.** The structured field `origin` is reserved. The bridge
  sets `origin = "tracing"` unless the event already carries an `origin`
  field (`"sqlite"` from the storage adapter, `"timeline"` from the timeline
  writer, `"graft"` from the binding). Events with `origin ∈ {"sqlite",
  "timeline"}` are retained in JSONL only and never fanned out to the SQLite
  timeline. A thread-local reentrancy guard discards (and counts) any event
  raised while the bridge is emitting. Both rules are stated identically in
  AW.1 and AW.2.
- **Redaction allowlist, not denylist.** `RETAINED_FIELD_ALLOWLIST` (defined
  in AW.1, reused by AW.2/AW.4): `ts, level, component, code, action,
  correlation_id, outcome, elapsed_ms, attempt, strategy, endpoint_kind,
  failure_class, refresh_error_code, error_layer, origin, message, detail`.
  Anything else is
  dropped. Never message bodies, template values, recipients, chat IDs,
  credentials, tokens, raw env/config values, absolute user paths.
- **Bounded everywhere.** Queues, batch sizes, row counts, detail length, and
  file sizes are explicit named constants with tests at the boundary.
- **Honest loss semantics.** Documentation and `atm log` output never claim
  lossless equivalence between JSONL and SQLite under overload.
- **sc-observability stays pinned.** `Cargo.toml` keeps
  `sc-observability = "=1.2.0"` and `sc-observability-types = "=1.2.0"`;
  each sprint's required validation includes
  `grep -n 'sc-observability' Cargo.toml` showing both `=1.2.0` lines
  unchanged, and the PR diff contains no `Cargo.toml` change to either line.
  A concrete API gap is recorded as a finding, never worked around by fork
  or vendoring.
- **Boundary records are definite and enforceable.** Every new or changed
  crate edge is named in the sprint doc with the exact boundary file and the
  exact `.just/lint-config.toml` `manifest_dependency_allowlists` entry to
  change. `forbidden_edges` entries are crate-level only (`<crate> ->
  <crate>`, the form `FORBIDDEN_EDGE_RE` in `.just/lint_boundaries.py`
  matches and `collect_forbidden_edge_violations` enforces); module-level
  restrictions are expressed as source-scan assertions added to
  `crates/atm-architecture/tests/boundary_enforcement.rs`, never as
  unenforced TOML text. A sprint grants an `allowed_dependents` /
  `allowed_dependencies` entry only in the sprint that actually adds the
  Cargo edge.

### Cross-sprint contracts (defined once, cited by sprint docs)

- **`DiagnosticSink`** (defined in AW.1, `crates/atm-observability/src/tracing_bridge.rs`;
  implemented in AW.2):

  ```rust
  /// One allowlisted, already-redacted event as the bridge saw it.
  pub struct RetainedEvent<'a> {
      pub ts_unix_ms: i64,
      pub level: sc_observability_types::Level,
      pub component: &'a str,          // tracing target
      pub code: Option<&'a str>,
      pub correlation_id: Option<&'a str>,
      pub origin: &'a str,             // "tracing" | "sqlite" | "timeline" | "graft"
      pub message: &'a str,
      pub fields: &'a [(&'static str, FieldValue)], // RETAINED_FIELD_ALLOWLIST members only
  }
  pub enum SinkOffer { Accepted, Dropped(DropReason) }
  pub enum DropReason { QueueFull, Disabled, PersistFailed }
  pub trait DiagnosticSink: Send + Sync {
      /// Must not block, allocate unboundedly, or emit tracing events.
      fn offer(&self, event: &RetainedEvent<'_>) -> SinkOffer;
  }
  ```

  The bridge calls `offer` after JSONL emission for every retained event
  whose `origin` is not `"sqlite"` or `"timeline"`; `Dropped` is counted in
  `TracingBridgeStats::sink_dropped_total` and never retried.
- **`DiagnosticCounters`** (defined in AW.1, `crates/atm-core/src/observability_counters.rs`,
  plain `Copy` snapshot struct with the JSONL and timeline counters named in
  AW.3 D1) plus `trait DiagnosticCountersSource: Send + Sync { fn snapshot(&self)
  -> DiagnosticCounters; }`. AW.1 implements it for `TracingBridgeStats`,
  AW.2 for the combined bridge+timeline stats, and the bootstrap injects one
  `Arc<dyn DiagnosticCountersSource>` into `RuntimeHealth` (AW.3) — so
  `atm-http-runtime` gains no crate edge.
- **`DiagnosticTimelineStore`** (defined in AW.2, `crates/atm-storage/src/diagnostics.rs`)
  is reached by AW.3 through `atm-runtime` router state exactly like the
  existing `MessageStore`/`RosterStore` handles; `atm-http-runtime` never
  imports `atm-storage-rusqlite`.

## 3. Sprints

| Sprint | Doc | Scope |
|---|---|---|
| AW.1 | [sprint-AW.1-tracing-bridge.md](./sprint-AW.1-tracing-bridge.md) | Process-wide `tracing` → sc-observability bridge layer for the daemon, structured field preservation, reentrancy guard, retirement of live-runtime `eprintln!`, documented pre-bootstrap stderr exceptions, shared observability path constants, new `boundaries/atm-observability/` record. |
| AW.2 | [sprint-AW.2-sqlite-diagnostic-timeline.md](./sprint-AW.2-sqlite-diagnostic-timeline.md) | `diagnostic_events` table, low-priority diagnostic channel on the existing writer lane, retention/pruning, `DiagnosticTimelineStore` contract, production `SqliteObservability` adapter replacing `NullSqliteObservability`, saturation/transition diagnostics, drop counters. |
| AW.3 | [sprint-AW.3-health-and-log-query.md](./sprint-AW.3-health-and-log-query.md) | Drop/degradation counters in health and doctor output, `atm log --source jsonl|timeline|merged` with timestamp-ordered merge over canonical JSONL + graft fallback satellite + SQLite timeline, retained-log contract documentation rewrite. |
| AW.4 | [sprint-AW.4-graft-fallback-observability.md](./sprint-AW.4-graft-fallback-observability.md) | Rust-owned observability API on the `atm-graft-python` Maturin binding: path diagnostics, fallback satellite emitter, event contract on daemon-unavailable/recovery paths for list/read/send/ack, envelope diagnostic on fallback write failure, Python 3.11–3.14 tests. |
| AW.5 | [sprint-AW.5-native-tool-parity.md](./sprint-AW.5-native-tool-parity.md) | #952: binding result types expose the full `atm-core` list/read/send outcomes; hermes-atm projections become pass-throughs; key-for-key parity test against the CLI `--json` output. |

Dispatch order under the branch model above:

```
AW.1 ──merged──▶ { AW.2 ∥ AW.4 } ──both merged──▶ AW.3
                        AW.4 ──merged──▶ AW.5   (AW.5 ∥ AW.3)
```

## 4. Execution notes

- Provisioning is task **AW.0** (team-lead): create `integrate/phase-aw`
  from `develop` after this plan merges; sprint branches are cut from
  `integrate/phase-aw` at dispatch time, never earlier, so each starts from
  its merged prerequisites.
- Each sprint: dev agent implements on its branch, PR opened on first push
  targeting `integrate/phase-aw`, `qa-pr<N>-r1` dispatched to quality-mgr
  immediately, CI is a merge gate only.
- Merge-forward from `integrate/phase-aw` on every parent merge
  (`feedback_merge_forward_asap`).
- **Execution deviation (recorded 2026-09-05):** AW.3 was cut and reviewed
  before AW.2 merged, violating the declared `AW.2 → AW.3 must_follow`
  relation. Merge-forward from `integrate/phase-aw` after AW.2 (PR #1179,
  #1195) and AW.4 landed substituted for the ordering; AW.3 (PR #1182, #1196, #1198)
  was re-verified by quality-mgr on the merged base before its final merge.
- Hermes-side (`crates/hermes-atm`, Python) is in-repo and owned by AW.4
  (fallback envelope consumption) and AW.5 (projections); no Hermes logger
  or path logic is ever written in Python.
- Storage architecture rule: AW.2 must not add a second SQLite writer
  connection; diagnostic writes flow through the existing single writer
  lane (`crates/atm-storage-rusqlite/src/writer/`) via a second, lower
  priority channel (mechanism specified in AW.2 deliverable 3) and AW.2
  records that policy as an amendment to `ADR-ATM-RUSQLITE-002`.
- **File reservations for parallel sprints** (a sprint must not edit a
  path reserved by its parallel partner; reviewers check the PR diff):

  | Sprint | Owns (exclusive while in flight) | Must not touch |
  |---|---|---|
  | AW.2 | `crates/atm-storage/src/diagnostics.rs`, `crates/atm-storage-rusqlite/**`, `crates/atm-daemon-bootstrap/src/{diagnostic_timeline.rs,daemon_sqlite_observability.rs}`, `docs/adr/ADR-ATM-RUSQLITE-002.md` | `crates/atm-graft-python/**`, `crates/hermes-atm/**`, `boundaries/atm-graft-python/**` |
  | AW.4 | `crates/atm-graft-python/**`, `crates/hermes-atm/**`, `boundaries/atm-graft-python/**`, `docs/graft-observability.md` | everything AW.2 owns |
  | AW.3 | `crates/atm/src/commands/log.rs`, `crates/atm/src/output.rs` (log/doctor rendering only), `crates/atm/tests/cli_surface_baseline.json`, `crates/atm-http-runtime/src/{runtime_health.rs,diagnostics_route.rs}`, `crates/atm-core/src/doctor/**`, `docs/atm-daemon/logging.md`, `docs/graft-observability.md` (cross-links only) | `crates/atm-graft-python/**`, `crates/hermes-atm/**`, `crates/atm-core/src/commands/**` |
  | AW.5 | `crates/atm-graft-python/**`, `crates/hermes-atm/**`, `boundaries/atm-graft-python/hermes-graft-binding.toml` | `crates/atm/**`, `crates/atm-http-runtime/**`, `docs/atm-daemon/**` (the parity test invokes the built `atm` binary; it does not edit CLI code) |

## 5. Out of scope (phase non-goals, from the issues)

- Persisting business messages or high-volume info telemetry in SQLite.
- Replacing JSONL with SQLite.
- New socket/queue/spool services; Hermes-specific log roots or files.
- Automatic replay of failed graft sends.
- New CLI fields or scoping-rule changes for list/read/send (#952 is
  parity, not extension).
- Any legacy synchronous daemon change.

## 6. QA history

### Round 1 — qa-pr1137-plan-r1 (2026-09-03, plan @ 811c212dd) — FAIL

7 reviewers (boundary-guard, ruthless-boundary-qa, plan-scope-reviewer,
critical-plan-reviewer, req-qa; plus ruthless-boundary-qa and req-qa on the
AW.5 supplement). Findings and dispositions (all addressed in round 2 text):

| # | Finding | Disposition |
|---|---|---|
| 1 | Blocking — AW.4 frontmatter listed AW.1 in both `must_follow` and `parallel_safe` | Fixed: branch model rewritten (§frontmatter `branch_model`); AW.4 `must_follow: [AW.1]`, `parallel_safe: [AW.2]` |
| 2 | Blocking — writer-lane priority mechanism undefined | Fixed: AW.2 D3 specifies a second bounded channel drained by the writer loop only when the primary channel is empty (`biased` select), one batch per idle tick |
| 3 | Blocking — `origin` recursion-break contradiction between AW.1 and AW.2 | Fixed: phase invariant "No recursion" defines the reserved `origin` field once; AW.1 D1 and AW.2 D5/D6 reference it verbatim |
| 4 | Blocking — checklist mapping unverifiable (reviewer had no gh) | Fixed: Appendix A quotes every #905/#904 checkbox and #952 expected-behaviour bullet with ids; sprint ACs cite ids |
| 5 | Blocking — no `boundaries/atm-observability/*.toml` for the new crate edge | Fixed: AW.1 D6 creates `boundaries/atm-observability/tracing-bridge.toml` and the matching `manifest_dependency_allowlists` entry |
| 6 | Important — AW.4 branch base vs parallel_safe tension | Fixed by the branch model (no stack; AW.4 cut from integrate after AW.1 merges) |
| 7 | Important — `RETAINED_INFO_TARGETS` undefined | Fixed: AW.1 D1 lists the constant's initial contents |
| 8 | Important — version-pin AC not testable | Fixed: phase invariant states the exact grep and diff condition |
| 9 | Important — AW.4 `endpoint_kind` sourcing mismatch | Fixed: sourced from `atm_daemon_client::LocalDaemonTransport` (`unix_domain_socket|tcp_loopback`), plus `failure_class` from the recovery classifier |
| 10 | Important — AW.5 ACs did not cite #952 items | Fixed: AC ids cite E1–E3 / items 1–3 |
| 11 | Important — AW.5 claimed `atm_read` never returns the body | Fixed: problem statement corrected (body returned when selected; gap is envelope + metadata) |
| 12 | Important — AW.5 referenced a non-existent `.pyi` stub | Fixed: removed; deprecation documented in hermes-atm README and via `DeprecationWarning` |
| 13 | Important — AW.5 boundary language hedged; edge mischaracterised as new | Fixed: stated definitively — no new edge (`atm-core` already allowlisted); `boundaries/atm-graft-python/hermes-graft-binding.toml` `[contracts].response_types` updated |
| m1 | Minor — call-site count discrepancy (51) | Fixed: 41 with the exact grep shown |
| m2 | Minor — AW.5 D1 claimed `output.rs` builds JSON inline | Fixed: D1 rewritten around the existing `atm-core` outcome structs |
| m3 | Minor — AW.5 AC2 "out of scope" undefined | Fixed: defined as "not visible to the caller under the read selection rules" |
| m4 | Minor — stale requirements.md version reference | Not found in the plan text on re-read; if the reviewer meant `docs/requirements.md` itself, it is outside this PR — flagged for round 2 to confirm |

### Round 2 — qa-pr1137-plan-r2 (2026-09-03, plan @ e75e88a96) — FAIL

5 reviewers on AW.1–AW.4 (report
https://github.com/randlee/atm-core/pull/1137#issuecomment-5519048194) plus
an AW.5 supplemental leg; consolidated report
https://github.com/randlee/atm-core/pull/1137#issuecomment-5519109942.
Round-1 fixes independently confirmed. New findings and dispositions (all
addressed in round 3 text):

| # | Finding | Disposition |
|---|---|---|
| B1 | Blocking — AW.4 added an `atm_ack` native tool with no happy-path AC or hermes exposure plan | Fixed: AW.4 D3a defines `atm_ack` as a thin tool over the existing acknowledgement path (`send_tool` with `acknowledges_message_id`), with request model, hermes registration, and happy-path/error-parity ACs (AC10–AC12). It stays in scope because #904-2 names `atm_ack` explicitly |
| B2 | Blocking — `failure_class: daemon_starting` had no classifier | Fixed: class removed. `failure_class ∈ {stale_client, endpoint_unavailable}` derived from the existing `with_daemon_recovery` outcomes (AW.4 D3), which is exactly the distinction #904-3 asks for; `refresh_error_code` carries the detail |
| B3 | Blocking — module-qualified `forbidden_edges` entry unenforced | Fixed: phase invariant now requires crate-level entries only; AW.1 D6 uses `atm-daemon -> atm-observability` (enforced by `collect_forbidden_edge_violations`) plus a source-scan test in `boundary_enforcement.rs` (AW.1 AC8) |
| I4 | AW.2 D3/D4 capacity/batching contradiction | Fixed: the channel carries batches; `DIAGNOSTIC_QUEUE_BATCHES = 8` × `DIAGNOSTIC_BATCH_MAX = 128` = 1024 events in flight; AC7/AC9 restated on those constants |
| I5 | Writer-lane policy amends ADR-ATM-RUSQLITE-002 without a record | Fixed: AW.2 D9 appends a "Phase AW amendment" section to the ADR |
| I6 | `docs/atm-daemon/logging.md:118` says sc-observability 1.0.0 | Fixed: AW.1 D5 rewrites that line; AC6 asserts no `1.0.0` remains in `logging.md` or `docs/requirements.md:907` (closes round-1 m4 / round-2 #11 too) |
| I7 | `DiagnosticSink` never specified | Fixed: signature in §2 "Cross-sprint contracts" |
| I8 | AW.3/AW.5 file overlap not reserved | Fixed: §4 file-reservation table |
| I9 | AW.1 D6 hedge; AW.3 health block crossed an unrecorded crate edge | Fixed: D6 lists exact contents; AW.3 uses `DiagnosticCountersSource` injected by bootstrap and reaches the timeline via `atm-runtime` state — no new edge |
| I10 | `atm-graft-python` grant in tracing-bridge.toml broader than needed | Fixed: AW.1 grants `allowed_dependents = ["atm-daemon-bootstrap", "atm"]` only; AW.4 amends the record when it adds the edge |
| I11 | ATM-QA-006 — AW.5 D2 claimed `atm_ack` parity without a test or AC | Fixed: AW.5 D3 parity test adds the ack case; AC6 asserts `atm_ack` equals `atm ack --json` |
| I12 | RBQA-AW5-F002 — ack result type unnamed, no `response_types` entry | Fixed: `AtmAckResult` (= `AtmSendResult`) named in AW.4 D3a and committed in AW.4 D5; AW.5 D4 re-asserts the full list |
| M13 | Minor — `docs/requirements.md:907` stale version | Fixed in AW.1 D5 (see I6) |
| M14 | RBQA-AW5-F001 — AW.5 D4 `response_types` commitment not mechanically checkable | Partially fixed in round 3 (`response_types` list only; `request_types` still elliptical); completed in round 4 (see R4-2) |
| CI | `just lint` spell failure on the plan branch (two misspelled words, now corrected) | Fixed in this round's commit |

### Round 3 — qa-pr1137-plan-r3 (2026-09-03, plan @ 517b5518f) — FAIL

6 reviewers dispatched; report
https://github.com/randlee/atm-core/pull/1137#issuecomment-5519248812.
All 16 round-2 items reconfirmed fixed. New findings and dispositions (all
addressed in round 4 text):

| # | Finding | Disposition |
|---|---|---|
| R4-1 | Important — §2 "exactly one sprint" rule contradicted by Appendix A composite ids (905-6, 905-8, 904-4, 904-7) | Fixed: §2 rule restated as "at least one sprint, every facet verified"; Appendix A already lists each claiming sprint and facet |
| R4-2 | Important — AW.5 D4 cites the pre-AW.4 `allowed_dependencies` baseline and leaves `request_types` elliptical; M14 was only half fixed | Fixed: AW.5 D4 cites the post-AW.4 baseline and states the complete `request_types` list; AC5 checks both lists verbatim; M14 disposition corrected |
| R4-3 | Important — `shared_db_reader_lanes.rs:43` cited as a production `NullSqliteObservability` site but is `#[cfg(test)]` | Fixed: §1 item 3 and AW.2 D5/AC8 now name only the two production sites (`lib.rs:741,750`) and exempt the test helper |
| R4-4 | Important — AW.4 D5 `response_types` wording imprecise vs AW.5 D4's split | Fixed: AW.4 D5 states `response_types` gains `AtmAckResult` and `PyObservabilityPaths`; `request_types` gains `AtmAckRequest` |
| R4-5 | Reviewer-set gap — plan-scope-reviewer needs a plan-hardening handoff artifact that a standalone QA dispatch does not produce | Process gap, not plan content; rounds 1–2 used the five-reviewer set. Round 4 dispatch names the five reviewers and records the gap explicitly |
| R4-6 | Minor — B3 disposition mis-cited the source-scan AC | Fixed: cites AW.1 AC8 |
| R4-7 | Minor — deliverable/AC numbering out of sequence in AW.1/AW.2 | Fixed: AW.1 AC9/AC10 and AW.2 D8/D9, AC10/AC11 reordered |
| R4-8 | Minor — AW.4 D3a `models.py` path lacked the crate prefix (PLAN-CRIT-M2) | Fixed |
| R4-9 | Minor — one further req-qa wording nit not quoted in the consolidated report | Open: round-4 dispatch asks req-qa to quote the exact text so it can be fixed |

### Round 4 — qa-pr1137-plan-r4 (2026-09-03, plan @ 2067165f8) — PASS

5 reviewers (req-qa, arch-qa, ruthless-boundary-qa, critical-plan-reviewer,
boundary-guard); report
https://github.com/randlee/atm-core/pull/1137#issuecomment-5519328683.
R4-1..R4-8 reconfirmed fixed by two or more reviewers each. R4-9 closed as
a phantom: req-qa's round-3 minors were exactly the three already handled
as R4-6, R4-7 and the spell fix. CI `just lint` green at 2067165f8. No
blocking, important or unresolved minor findings remain. The plan gate is
passed; Phase AW provisioning, dev dispatch and the merge of this PR stay
on hold until team-lead confirms the release has shipped.

## Appendix A — Issue checklist ids (verbatim)

### #905 acceptance criteria

- **905-1** An induced runtime `tracing::warn!` and `tracing::error!` from the Tokio/Axum path each appear in the retained `sc-observability` JSONL file with level, code, component, and correlation metadata. → AW.1 AC1
- **905-2** Post-bootstrap daemon warning/error paths no longer rely solely on `eprintln!`; documented pre-bootstrap exceptions remain stderr-visible. → AW.1 AC6
- **905-3** SQLite contains a compact diagnostic-event record for minor diagnostic events selected by policy and for every warning/error emitted through the unified pipeline. → AW.2 AC2
- **905-4** SQLite persistence is bounded, redacted, pruned, and cannot block or alter message delivery, acknowledgement, or database-write outcomes. → AW.2 AC3, AC4, AC5, AC9
- **905-5** SQLite writer/WAL failures, queue saturation, and checkpoint failures produce retained structured diagnostics rather than disappearing through `NullSqliteObservability`. → AW.2 AC6, AC8
- **905-6** Sink degradation/drop counters are exposed in health output and produce a rate-limited retained diagnostic when a transition occurs. → AW.2 AC7 (diagnostic), AW.3 AC1 (health)
- **905-7** `atm log` can query the canonical JSONL stream and exposes the SQLite diagnostic timeline through a clearly documented query mode or merged view, without falsely claiming lossless equivalence under bounded sink overload. → AW.3 AC2, AC3, AC5
- **905-8** Tests cover redaction, structured field preservation, source-level filtering, SQLite retention/pruning, logging failure non-interference, queue saturation, recursion prevention, and #904 fallback event merge/order. → AW.1 AC1–AC5 (fields, filtering, recursion, non-interference, redaction); AW.2 AC3, AC4, AC7 (non-interference, retention, saturation); AW.3 AC3 (merge/order)
- **905-9** Documentation is updated so the retained-log contract precisely states guarantees, exceptions, and loss/degradation behavior. → AW.1 AC6 (allowlist doc), AW.3 AC6

### #904 acceptance criteria

- **904-1** Maturin bindings expose canonical log directory/path diagnostics derived by Rust. → AW.4 AC1
- **904-2** A simulated unavailable daemon during a native `atm_list`, `atm_read`, `atm_send`, or `atm_ack` call creates a redacted structured event in the fallback satellite log. → AW.4 AC2, AC3
- **904-3** Native reconnect events include enough structured metadata to distinguish first-call stale-client failure from endpoint/daemon startup failure. → AW.4 AC2 (`failure_class`)
- **904-4** `atm log` returns canonical and fallback events in one timestamp-ordered result. → AW.3 AC3
- **904-5** The daemon is the only process performing rotation/retention for `atm.log.jsonl`; fallback retention/rotation is isolated and safe. → AW.4 AC5, AC6
- **904-6** A fallback-sink write failure is observable in the native JSON envelope without masking the original operation outcome. → AW.4 AC4
- **904-7** Tests cover redaction, environment-based log-root resolution, daemon-down logging, query merge/order, logger-failure diagnostics, and concurrent daemon/graft activity. → AW.4 AC1, AC3, AC2, AC4, AC7; AW.3 AC3 (merge/order)
- **904-8** Python 3.11–3.14 binding test coverage remains green. → AW.4 AC8

### #952 expected behaviour (no checkboxes in the issue; ids assigned here)

- **952-E1** `atm_list` native result should include the CLI-equivalent envelope (action/team/agent/selection_mode/history_collapsed/bucket_counts) and use `from` (or at minimum expose both). → AW.5 AC1, AC4
- **952-E2** `atm_read` should return the full message JSON (body included) — reading from the daemon store when the message is visible to the caller. → AW.5 AC1, AC2
- **952-E3** `atm_send` should return the CLI-equivalent envelope with sender/summary metadata. → AW.5 AC1
- Evidence items 1–3 (field name, envelope, timestamp serialisation) → AW.5 AC1, AC3
