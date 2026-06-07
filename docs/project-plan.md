# ATM CLI Project Plan

## 1. Goal

Implement the retained ATM CLI surface while migrating mail/runtime ownership
from filesystem JSON plus mailbox locks to SQLite plus a singleton daemon,
preserving `send`, `read`, `ack`, `clear`, `log`, `doctor`, `teams`, and
`members`.

The authoritative migration document is:
- [`docs/archive/file-migration-plan.md`](./archive/file-migration-plan.md)

This plan sequences the work. File-level migration decisions live in
[`docs/archive/file-migration-plan.md`](./archive/file-migration-plan.md).

Documentation organization and cleanup are governed by
[`documentation-guidelines.md`](./documentation-guidelines.md). As the docs are
restructured, product docs remain in `docs/` and crate-local detail moves into
`docs/atm/`, `docs/atm-core/`, `docs/atm-daemon/`, and
`docs/atm-rusqlite/`.

Phase-Q disposition note:
- earlier daemon-free phases in this plan remain historical execution records
- The former early SQLite/daemon line is abandoned as an implementation line
- `docs/plans/phase-Q/plan-phase-Q.md` and Section 21 are retained as minimal historical
  execution records only
- any retained value from that abandoned line must be brought forward manually after review

Phase-R redesign note:
- the next execution line is the Phase R redesign and enforcement pass tracked
  in [`docs/plans/phase-R/plan-phase-R.md`](./plans/phase-R/plan-phase-R.md)
- Phase R starts with boundary documents, ADR alignment, and lint/parser gates
  before new implementation work
- the active integration branch for this redesign line is `integrate/phase-R`

Phase-S planning note:
- Phase R is the merged daemon baseline, but it missed the requirement that the
  full daemon feature set must work on Windows as well as Unix-like hosts
- the active planning line for that correction is Phase S, tracked in
  [`docs/plans/phase-S/plan-phase-S.md`](./plans/phase-S/plan-phase-S.md)
- the canonical daemon wire contract, current daemon packet surface, and shared
  local-IPC/host-host frame rules are tracked in
  [`docs/atm-daemon/protocol-icd.md`](./atm-daemon/protocol-icd.md)
- Phase S is not satisfied by Windows compilation or temporary unsupported-path
  stubs; it closes only when daemon functionality is production-ready on every
  supported operating system behind the documented portability boundaries
- Phase S implementation details must come either from `docs/plans/phase-S/plan-phase-S.md`
  or from the governing requirements, architecture, ADR, and ICD documents it
  names; the project plan does not override those lower-level sources of truth
- the planning baseline is `integrate/phase-R` at `6a072c1`
- S.5 is the follow-on planning slice that tightens the no-flaky-test policy,
  defines which anti-flake guardrails belong in the default lint path, and
  documents the bounded queue-query split between `atm list` and
  single-message `atm read`, including the ATM-authored Claude JSONL
  compatibility envelope for oversized message bodies
- the remaining Phase S implementation work continues in:
  - `S.6` daemon post-mortem runtime remediation
  - `S.7` bounded queue-query implementation
  - `S.8` Claude JSONL compatibility-envelope implementation
  - `S.9` host-scoped retained logging defaults, including watcher/reconcile
    exclusion for `~/.atm/logs/`

Phase-AA simplification note:
- after the retained daemon/SQLite line proved the transport split, the daemon
  accumulated concrete SQLite composition and health/observability ownership
  that violated the intended boundary
- the corrective planning line is Phase AA, tracked in
  [`docs/plans/phase-AA/plan-phase-AA.md`](./plans/phase-AA/plan-phase-AA.md)
- Phase AA restores the original daemon role as a thin router by moving
  concrete SQLite construction to a dedicated `atm-runtime` crate and
  restoring a direct local doctor/store-health path

Phase-AB planning note:
- `Phase AB` is the active cross-host smoke planning line that follows the
  completed same-host release-readiness work in `Phase Z`
- the authoritative planning document is
  [`docs/plans/phase-AB/plan-phase-AB.md`](./plans/phase-AB/plan-phase-AB.md)
- `Phase AB` owns Windows/macOS real-binary cross-host smoke coverage on
  disposable clean-room state first, then disposable copied-state revalidation
- the planning branch is `plan/phase-AB`
- the execution integration branch is `integrate/phase-AB`

Phase R execution entry:
- Wave 1 deliverable: the new Phase R skeleton
  - new crates
  - public boundary traits/facades
  - major data structures
- Wave 1 supporting sequence:
  1. `R.0` lint foundation
  2. `R.1` lint debt burn-down
  3. `R.2` skeleton crates, boundary traits/facades, and major data structures
  4. `R.2A` parallel lint hardening
- `R.3` is a dedicated review/re-planning stage after the Wave 1 skeleton lands
- Wave 2 executes implementations only against the enforced boundary skeleton

Status:
- Phases 0 through P have executed on the retained rewrite line.
- Phases G and H are complete retained-command phases, closed through the
  shared observability and release-alignment work delivered in later phases.
- Phase K completed the shared `sc-observability` integration boundary.
- Phase L completed the retained release-surface and team-recovery closeout.
- Phase M completed mailbox locking and review-finding fixes.
- Phase N completed publish-replacement and distribution-parity planning and
  implementation merge work.
- Phase O completed the security and hardening follow-up line.
- Phase P implementation is merged; follow-up hardening remains open for
  `P.6` and later cleanup/fix branches, while `P.8` documentation
  reconciliation and the `P.9`/`P.10` lock-sentinel design and implementation
  work are complete on the merged Phase P line.
- Message schema ownership and metadata normalization are now implemented well
  enough for live shared-inbox adoption, while a separate ATM-native inbox
  remains deferred to a later version.
- The former early SQLite/daemon line is retained only as an abandoned historical attempt at the SQLite
  source-of-truth and daemon-boundary redesign.
- Phase R is the merged daemon baseline.
- Phase S is the active planning line for Windows-complete daemon parity.
- Phase AA is the architectural simplification planning line for removing
  SQLite references from `atm-daemon` and moving concrete runtime assembly out
  to `atm-runtime`.
- Phase AB is the active planning line for Windows/macOS cross-host ATM smoke
  execution after the accepted Phase Z baseline.
- the current merged workspace contains:
  - `crates/atm-architecture`
  - `crates/atm-core`
  - `crates/atm`
  - `crates/atm-daemon`
  - `crates/atm-daemon-client`
  - `crates/atm-graft`
  - `crates/atm-runtime`
  - `crates/atm-rusqlite`
  - `crates/sc-lint-*` support crates

## 2. Deliverables

- Rust workspace expanded from `crates/atm-core` + `crates/atm` to include
  `crates/atm-daemon` and `crates/atm-rusqlite`
- retained implementation of `send`, `read`, `ack`, `clear`, `log`,
  `doctor`, `teams`, and `members`
- SQLite-backed mail and roster source of truth
- singleton daemon runtime with one protocol, two production transport
  adapters, and one in-process `test-socket`
- elimination of mailbox-lock dependence from ATM mail correctness
- explicit two-axis workflow model with three display buckets
- task-linked message metadata with mandatory ack behavior
- structured errors with recovery guidance
- structured logs through `sc-observability`
- retained and new integration tests for the retained command surface
- explicit schema ownership docs for Claude Code, legacy ATM compatibility, and
  forward ATM metadata

## 3. Crates

The abandoned early SQLite/daemon target implementation was split across:

- `crates/atm-core`
- `crates/atm`
- `crates/atm-daemon`
- `crates/atm-daemon-client`
- `crates/atm-graft`
- `crates/atm-rusqlite`

Crate-local scope detail is owned by:

- [`docs/atm-core/requirements.md`](./atm-core/requirements.md)
- [`docs/atm-core/architecture.md`](./atm-core/architecture.md)
- [`docs/atm-core/boundaries.md`](./atm-core/boundaries.md)
- [`docs/atm/requirements.md`](./atm/requirements.md)
- [`docs/atm/architecture.md`](./atm/architecture.md)
- [`docs/atm/boundaries.md`](./atm/boundaries.md)
- [`docs/atm-daemon/requirements.md`](./atm-daemon/requirements.md)
- [`docs/atm-daemon/architecture.md`](./atm-daemon/architecture.md)
- [`docs/atm-daemon/boundaries.md`](./atm-daemon/boundaries.md)
- [`docs/atm-rusqlite/requirements.md`](./atm-rusqlite/requirements.md)
- [`docs/atm-rusqlite/architecture.md`](./atm-rusqlite/architecture.md)
- [`docs/atm-rusqlite/boundaries.md`](./atm-rusqlite/boundaries.md)

Phase R sequencing rule:
- no new implementation sprint begins until:
  - the relevant boundary records exist
  - architecture/requirements/ADR docs agree with those records
  - the parser/lint pass for those records is in place
- Phase R implementation proceeds in this order:
  - boundary design
  - document alignment
  - lint/parser gates
  - skeleton implementation
  - feature behavior

## 4. Work Sequence

### Phase AA: Remove SQLite From Daemon [PLANNED]

Status summary:
- Phase AA is the active simplification planning line for restoring
  `atm-daemon` to a thin-router role.
- Integration Branch: `integrate/phase-AA`
- The authoritative plan is [`docs/plans/phase-AA/plan-phase-AA.md`](./plans/phase-AA/plan-phase-AA.md).
- The authoritative closure checklist is
  [`docs/plans/phase-AA/readiness.md`](./plans/phase-AA/readiness.md).
- `AA.0` completed the daemon-role restatement, top-level state-machine
  inventory, and daemon-side SQLite leak ledger that later AA sprints must
  follow.
- `AA.1` completed the subsystem-owned doctor traits and shared diagnostic DTO
  move into `atm-core`.
- `AA.2` completed the `atm-runtime` composition-root introduction, moved
  production SQLite/runtime assembly out of daemon production composition, and
  froze the target runtime boundary while the SQLite TOML relock remains
  deferred to `AA.5`.
- `AA.3` completed the direct-local doctor split and daemon runtime-health
  simplification so store diagnostics no longer require daemon-only routing.
- `AA.4` removes the remaining daemon-side SQLite leak paths by deleting the
  daemon-private SQLite observability adapter, deleting direct daemon test
  boundary assembly calls, and relying on `atm-core` / `atm-runtime` replay
  seams instead of a direct `atm-daemon -> atm-rusqlite` dependency.
- `AA.5` relocks the daemon-to-SQLite edge in the runtime and SQLite boundary
  TOMLs, adds the independent `crates/atm-architecture/` Rust review guard,
  and freezes boundary-policy widening as an explicit architecture change.
- `AA.6` completes the scoped `sc-observability` `1.2.0` migration by moving
  the concrete adapters to queue-backed `Logger::log()` admission, renaming
  the retained-log shutdown policy field to `writer_shutdown_timeout`, and
  projecting queue/writer/maintenance health detail intentionally.
- `AA.7` Rust Boundary Enforcement Crate (`PR #398`,
  `feature/pAA-s7-atm-architecture-crate`) completes the visible workspace
  architecture gate by landing `crates/atm-architecture/`, removing the
  superseded Python boundary scripts, and making `cargo test -p atm-architecture`
  the sole code-driven boundary-enforcement check. Status: `complete`.
- `AA.8` Claude Code Inbox Schema Contract Alignment
  (`feature/pAA-s8-claude-schema-contract`) is complete: the current Claude
  Code inbox JSON contract is frozen from real `team-lead -> quality-mgr`
  samples, schema-model fixtures cover those shapes, and docs/models no longer
  classify the current JSON-array inbox shape as legacy.
- `AA.9` Current Claude Inbox Primary-Path Repair
  (`feature/pAA-s9-claude-inbox-primary-path`) is complete: the retained
  runtime now treats the current Claude inbox JSON file shape as the supported
  primary compatibility path, `.json` inboxes rewrite atomically as current
  Claude arrays, and the thorough smoke lane no longer expects compatibility
  degradation for a healthy current Claude inbox.
- `AA.10` Remove Historical ATM JSON Compatibility From 1.2
  (`feature/pAA-s10-remove-historical-atm-json`) is complete: historical
  ATM-owned inbox JSON is no longer presented as the active primary 1.2
  contract, while legal additive derivatives such as tolerated top-level ATM
  fields and `metadata.atm.*` remain read-compatible only and are ignored for
  active machine-state behavior.
- `AA.11` (`feature/pAA-s11-delete-sqlite-legacy-compat`) is complete:
  pre-production SQLite compatibility scaffolding such as `legacy_message_id`
  is no longer part of the active 1.2 runtime/bootstrap line, and surviving
  references remain only as historical inventory/ADR context.
- `AA.12` (`feature/pAA-s12-malformed-claude-inbox-recovery`) is complete:
  malformed Claude inbox reads now salvage segmentable valid messages, emit
  explicit degraded warnings for localized bad fragments, and keep rewrite
  paths fail-closed unless an explicit repair/rebuild action is chosen.

Goal:
- move concrete SQLite/runtime assembly to `atm-runtime`
- remove daemon-owned SQLite diagnostics, observability glue, and replay/store
  leakage
- relock the daemon-to-SQLite boundary with a permanent second enforcement
  layer

Deliverables:
- `crates/atm-runtime` as the concrete composition root
- subsystem doctor trait model and direct local doctor path
- deletion of remaining daemon-side SQLite leaks
- `boundary-guard` and relocked machine-readable boundary policy
- `sc-observability` / `sc-observability-types` upgraded to `1.2.0` with the
  queue-backed logger API, retained-log policy field migration, and updated
  health projection

Sprint line:
- `AA.0` `feature/pAA-s0-daemon-architecture-restatement`
- `AA.1` `feature/pAA-s1-subsystem-doctor-traits`
- `AA.2` `feature/pAA-s2-atm-runtime-composition-transfer`
- `AA.3` `feature/pAA-s3-direct-doctor-and-runtime-health-split`
- `AA.4` `feature/pAA-s4-delete-daemon-sqlite-leaks`
- `AA.5` `feature/pAA-s5-boundary-relock-and-permanent-enforcement`
- `AA.6` `feature/pAA-s6-obs-upgrade`
- `AA.7` `feature/pAA-s7-atm-architecture-crate`
- `AA.8` `feature/pAA-s8-claude-schema-contract`
- `AA.9` `feature/pAA-s9-claude-inbox-primary-path`
- `AA.10` `feature/pAA-s10-remove-historical-atm-json`
- `AA.11` `feature/pAA-s11-delete-sqlite-legacy-compat`
- `AA.12` `feature/pAA-s12-malformed-claude-inbox-recovery`

Acceptance:
- Phase AA exit criteria are satisfied only through
  `docs/plans/phase-AA/readiness.md`

### Phase AC: Storage Contract Reset And Backend Interchangeability [PLANNED]

Status summary:
- Phase AC is the planning line that restores the original storage and RPC
  design after the repo drifted into backend-shaped seams and per-operation
  request/response storage DTOs.
- Planning Branch: `plan/phase-AC`
- Integration Branch: `integrate/phase-AC`
- `AC.0` planning prerequisite is complete at `ce02b9ff`.
- latest accepted planning tip is the current `plan/phase-AC` branch head,
  which carries the full plan-hardening sequence, the
  [exhaustive AC.0 type ledger](./phase-AC/type-ledger.md), and the final
  cross-document consistency corrections for the AC sprint set.
- The authoritative plan is [`docs/plan-phase-AC.md`](./plan-phase-AC.md).
- The authoritative closure checklist is
  [`docs/phase-AC/readiness.md`](./phase-AC/readiness.md).

Goal:
- create a small audited `atm-storage` contract
- extract Claude inbox storage as a first-class backend
- converge the SQLite backend on that same contract
- collapse RPC/storage/domain type duplication back to canonical shared structs
- restore future SQL Server viability

Deliverables:
- `crates/atm-storage`
- `crates/atm-storage-claude`
- converged SQLite backend against the same core traits
- generic RPC envelope plus canonical shared domain bodies
- deletion of obsolete storage/RPC wrapper families

Sprint line:
- `AC.0` `plan/phase-AC` `complete`
- `AC.1` `feature/pAC-s1-atm-storage-contract-and-canonical-types`
- `AC.2` `feature/pAC-s2-atm-storage-claude-extraction`
- `AC.3` `feature/pAC-s3-sqlite-backend-convergence`
- `AC.4` `feature/pAC-s4-atm-core-storage-boundary-adoption`
- `AC.5` `feature/pAC-s5-rpc-envelope-and-domain-type-unification`
- `AC.6` `feature/pAC-s6-cleanup-and-deletion-closeout`
- `AC.7` `feature/pAC-s7-sqlserver-readiness-proof`

Acceptance:
- Phase AC exit criteria are satisfied only through
  `docs/phase-AC/readiness.md`

### Phase 0: Document Lock [COMPLETE]

- **Phase 0: Document Lock [COMPLETE]** — Locked requirements, architecture, and read-behavior documentation, and moved the migration plan to `docs/archive/`. (Completed before the current PR sequence; no dedicated PR.)

### Phase A: `OBS-GAP-1` [COMPLETE]

- **Phase A: `OBS-GAP-1` [COMPLETE]** — Catalogued and closed the `sc-observability` API gap before ATM depended on it for `atm log` and `atm doctor`. (Delivered in PR #1)

### Phase B: Core Skeleton [COMPLETE]

- **Phase B: Core Skeleton [COMPLETE]** — Created workspace, crate scaffolding, CLI command surface, and closed documentation gaps for the initial core messaging surface. (Delivered in PRs #2 and #3)

### Phase C: Low-Level Reuse [COMPLETE]

- **Phase C: Low-Level Reuse [COMPLETE]** — Landed foundational reuse for mailbox schema alignment, config/path helpers, and the shared `AtmError` / `AtmErrorKind` model. (Delivered in PRs #4 and #5)

### Phase D: Send Path [COMPLETE]

- **Phase D: Send Path [COMPLETE]** — Implemented the send service, CLI wiring, observability port adapter, and team-config validation. (Delivered in PR #6)

### Phase E: Read Path [COMPLETE]

- **Phase E: Read Path [COMPLETE]** — Implemented the read service with `IsoTimestamp`, seen-state handling, queue bucket filtering, and required read-path transitions. (Delivered in PR #7)

### Phase F: Ack And Clear Path [COMPLETE]

- **Phase F: Ack And Clear Path [COMPLETE]** — Implemented ack and clear flows, closed 30 RBP findings, and completed CI isolation hardening. (Delivered in PRs #8, #9, and #10)

### Phase G: Log Path [UNBLOCKED - Phase K COMPLETE]

- **Phase G: Log Path [UNBLOCKED - Phase K COMPLETE]** — Delivered the retained `log` command on the shared `sc-observability` query/follow stack after Phase K landed the real adapter. (Unblocked by Phase K; implemented as part of Phase K.4)

### Phase H: Doctor Path [UNBLOCKED - Phase K COMPLETE]

- **Phase H: Doctor Path [UNBLOCKED - Phase K COMPLETE]** — Delivered the retained `doctor` command on shared observability health/query integration after Phase K landed the real adapter. (Unblocked by Phase K; implemented as part of Phase K.5)

### Phase I: Cleanup And Hardening

- **Phase I: Cleanup And Hardening [COMPLETE]** — Deleted daemon-dependent helpers, added integration/snapshot tests, and hardened config/schema recovery for legacy team records. (Absorbed into later phases)

### Phase J: Message Schema Normalization [COMPLETE]

- **Phase J: Message Schema Normalization [COMPLETE]** — Locked schema ownership for Claude-native, legacy ATM read-compat, and forward ATM metadata fields; validated the shared-inbox design live; deferred a separate ATM-native inbox to a later version.

### Phase K: `sc-observability` Integration [COMPLETE]

- **Phase K: `sc-observability` Integration [COMPLETE]** — Integrated ATM with the shared `sc-observability` stack for retained emit, query, follow, and health; delivered `atm log` and `atm doctor` on the shared stack with ATM-owned boundary types. (Integration published via `K-CRATES-IO-1` crates.io cutover)

### Phase L: 1.0 Alignment And Release Surface Cleanup [COMPLETE]

- **Phase L: 1.0 Alignment And Release Surface Cleanup [COMPLETE]** — Completed published `sc-observability 1.0` follow-on work (stderr routing, fault injection, file sink migration, API cleanup, construction ergonomics, release closeout), team baseline/identity source cleanup, and retained team recovery surface (`teams`, `members`, `teams add-member`, `teams backup`, `teams restore`). (L.1-L.8 complete; merged to `integrate/phase-L`)

### Phase M: Mailbox Locking And Code Review Fixes [COMPLETE]

- **Phase M: Mailbox Locking And Code Review Fixes [COMPLETE]** — Implemented exclusive mailbox locking with deterministic sorted-path acquisition, closed all blocking BP-ECR-001–BP-ECR-006 code-review findings (error docs, recovery guidance, backtrace display, identity consolidation, panic removal, atomicity), and added the M.F1 locking hardening follow-up for fail-closed source discovery and read-only filesystem classification. (M.1 PR #60, M.2 PR #61; integrated to `develop`)

### Phase N: Publish Replacement And Distribution Parity [COMPLETE]

- **Phase N: Publish Replacement And Distribution Parity [COMPLETE]** — Switched publishable crate identities to `agent-team-mail` / `agent-team-mail-core`, ported release automation (crates.io, GitHub Releases, Homebrew), added `winget` as a new required Windows install channel, ported the publisher agent, rewrote README for release-facing docs, and proved dry-run publishability. (Sprints N.1–N.5; merged to `develop`)

### Phase O: Security And Hardening [COMPLETE]

- **Phase O: Security And Hardening [COMPLETE]** — Closed the four confirmed CR001 findings: path-segment validation for team/agent names, `normalize_json_number` expansion cap, UUID-based atomic temp-file naming, and sleep/backoff after stale-lock eviction. (Sprints O.1–O.2; integrated on `integrate/phase-O`)

### Phase P: File-I/O Ownership And Single-Write-Path Hardening [COMPLETE]

- **Phase P: File-I/O Ownership And Single-Write-Path Hardening [COMPLETE]** — Applied one explicit file-I/O ownership model (read_only / read_possible_write / read_modify_write) across every live file family, eliminated ad hoc write paths, introduced the ATM-owned workflow sidecar, completed lock-sentinel gap closure (P.9/P.10), and reconciled requirements/architecture docs with the landed implementation. (Sprints P.1–P.5, P.6–P.10, M.F1; PRs #111–#115, #120; integrated to `develop`)

## 5. Hard Rules

- Removing the daemon does not authorize removing retained mail functionality.
- File-level migration decisions must be explicit.
- Every retained useful source file must appear in
  `docs/archive/file-migration-plan.md`.
- Every reviewed non-retained file must also appear there with a `do not copy` decision.
- Workflow-axis transitions must be enforced by code structure, not only by tests.
- Display bucket behavior must remain separate from the canonical two-axis workflow model.
- Task-linked mail must be ack-required from creation time.
- Generic logging query/follow/filter behavior should live in `sc-observability` where possible, not in ATM-specific code.
- Persisted config/schema compatibility issues must recover at the narrowest
  safe scope, and identity/routing fields must never be guessed.
- Missing team config remains distinct from malformed team config; only the
  documented send fallback may bypass it, and repeated repair notifications
  must be deduplicated by unresolved condition.

Cross-document invariants that must stay locked during implementation:
- `taskId` implies ack-required send behavior
- displayed messages always persist `read = true`
- pending-ack messages remain actionable until acknowledged
- `atm clear` never removes unread messages
- `atm clear` never removes pending-ack messages
- `atm read --timeout` returns immediately when the requested selection is already non-empty

## 6. Done Definition

The rewrite is ready when:
- `atm send` works through the documented production runtime path
- `atm read` works through the documented production runtime path
- `atm ack` works through the documented production runtime path
- `atm clear` works through the documented production runtime path
- `atm log` works through shared observability APIs
- `atm doctor` works as a local diagnostics command with daemon/runtime
  visibility in the current SQLite/daemon architecture
- `atm teams` provides the retained local team recovery surface
- `atm members` provides retained local roster verification
- daemon auto-start-when-absent path is exercised in bounded integration
  testing
- `ATM_POST_SEND.recipient_pane_id` is sourced from SQLite roster truth when
  known
- retained command behavior is preserved, and any current-runtime shape changes
  are intentionally documented
- task-linked mail remains pending until acknowledged
- the file-by-file migration plan is complete enough to implement directly
- the retained command tests pass against the new crate layout

## 7. Documentation Review Checks

Before implementation starts, the docs should be reviewed with these checks:
- every retained or rejected source file referenced by the retained command
  surface appears in `docs/archive/file-migration-plan.md`
- `requirements.md`, `architecture.md`, and `read-behavior.md` agree on the two-axis model, three display buckets, and legal transitions
- `requirements.md`, `architecture.md`, and `read-behavior.md` agree on `--since`, `--since-last-seen`, `--no-since-last-seen`, `--no-update-seen`, and `--timeout`
- `requirements.md`, `architecture.md`, `docs/atm/requirements.md`, and
  `docs/atm/architecture.md` agree on the retained release surface:
  `send`, `read`, `ack`, `clear`, `log`, `doctor`, `teams`, `members`
- `docs/archive/file-migration-plan.md` remains the source of truth for the
  initial core migration set (`send`, `read`, `ack`, `clear`, `log`,
  `doctor`), and the release-only `teams` / `members` expansion is explicitly
  tracked in Phase `L.8`

## 21. Former Phase Q [ABANDONED]

- **Phase Q [ABANDONED]** — The former Phase Q SQLite/daemon execution line was abandoned; `docs/plans/phase-Q/plan-phase-Q.md` is retained as a one-line historical marker only, and any still-useful ideas must be brought forward manually into the active Phase R documents.

## 22. Phase R — Boundary Establishment And Enforcement [COMPLETE]

- **Phase R: Boundary Establishment And Enforcement [COMPLETE]** — Established enforceable crate boundaries, lint/parser foundation, new crate skeleton, public boundary traits/facades, and major shared data structures as Wave 1; implemented behavior against the enforced boundary in Wave 2. (Authoritative plan: [`docs/plans/phase-R/plan-phase-R.md`](./plans/phase-R/plan-phase-R.md); merged daemon baseline)

## 23. Phase R.9 / R.10 — Daemon Singleton And Test Fidelity Hardening [COMPLETE]

- **Phase R.9 / R.10: Daemon Singleton And Test Fidelity Hardening [COMPLETE]** — Made daemon singleton the first-class runtime invariant, removed daemon-spawn-driven test strategy from the correctness path, and replaced it with production-faithful in-process transport seams and narrow daemon-runtime coverage.

## 24. Phase R Postmortem Linter Backfill [COMPLETE]

- **Phase R Postmortem Linter Backfill [COMPLETE]** — Converted recurring mechanically-detectable Phase R defect families (Unix platform-gating, bare `Condvar::wait`, duplicate semantic string literals, fixed-sleep test hygiene, triage-record consistency) into repository lint or CI gates, with reusable rules staged for graduation to standalone `sc-lint`.

## 25. Phase U Mailbox Simplification And Identity Cleanup [COMPLETE]

- **Phase U: Mailbox Simplification And Identity Cleanup [COMPLETE]** — Removed legacy mailbox/identity carry-forward design, made SQLite the sole ATM-owned mailbox authority outside the Claude-compat watcher boundary, and replaced ambiguous message identity/state/thread-update behavior with smaller auditable contracts across sprints U.0–U.11. (Integration branch: `integrate/phase-U`)

## 26. Phase V Daemon Hardening And Boundary Cleanup [COMPLETE]

- **Phase V: Daemon Hardening And Boundary Cleanup [COMPLETE]** — Closed daemon hardening follow-on from Phase U: defined `SubsystemObservability` per-subsystem injection, deleted old central event-reconstruction helpers, and hardened `.with_recovery()` on the four required runtime error categories across sprints V.1–V.4. (PRs #269–#277 range via Phase W completion)

## 27. Phase W Production Readiness Follow-Up [COMPLETE]

- **Phase W: Production Readiness Follow-Up [COMPLETE]** — Closed remaining production-readiness gaps after Phase V: daemon-side sink-failure visibility, same-host traceability and interface parity, SQLite observability and protocol parity, peer replay recovery, doctor projection, SQLite error-contract cleanup, and phase closeout. (Sprints W.1–W.8; PRs #269–#277; integration branch `integrate/phase-W`)

## 28. Phase Xb SQLite SSOT And Daemon Boundary Simplification Restart [COMPLETE]

- **Phase Xb: SQLite SSOT And Daemon Boundary Simplification Restart [COMPLETE]** — Removed the dual mailbox/runtime implementation so ATM has one durable mailbox path, aligned daemon runtime truth with the SQLite SSOT claim, and made replay persistence startup behavior explicit and enforceable. (Integration branch: `integrate/phase-Xb`; authoritative plan: `docs/phase-X/plan-phase-X.md`)

## 29. Phase Xb Planning And Pre-Phase Lint Prerequisite [COMPLETE]

- **Phase Xb Planning And Pre-Phase Lint Prerequisite [COMPLETE]** — Added guardrails to catch stale legacy paths and silent regressions earlier; removed remaining legacy mailbox/runtime branches behind the retained boundary. (Pre-phase branch: `feature/pX-lint-gates`)

## 30. Phase Y Pre-Smoke Trivial Fixes [COMPLETE]

- **Phase Y Pre-Smoke Trivial Fixes [COMPLETE]** — Landed small pre-Phase-Y cleanup items: shared `ATM_SERVICE_NAME` reuse, `atm ack` validation cleanup, architecture wording, `GH #78` regression coverage, and trivial-fixes QA-1 follow-up. (Branch: `feature/pY-trivial-fixes`; status: complete)

## 31. Phase Y Daemon Release Readiness, Compatibility Write Simplification, And Smoke Rollout [COMPLETE]

- **Phase Y: Daemon Release Readiness, Compatibility Write Simplification, And Smoke Rollout [COMPLETE]** — Made the first daemon + SQLite mail-SSOT release safe for real operator use: consolidated compatibility writes behind one hard owner boundary, centralized delivery routing, removed mutable workflow-state projection from compatibility output, and delivered `atm help` UX improvements across sprints Y.1–Y.6. (Authoritative plan: `docs/plan-phase-Y.md`; integration branch: `integrate/phase-Y`)

## 32. Phase Yb Message-Path Consolidation Planning [COMPLETE]

- **Phase Yb: Message-Path Consolidation Planning [COMPLETE]** — Consolidated message paths after Phase Y: shared delivery plans across Claude/non-Claude harness paths, dedicated `NonClaudeOutbound` payload boundary, fail-closed handling for missing roster harness data, and repair/rebuild-only mailbox rewrite seams across sprints Y.7–Y.11. (Integration branch: `integrate/phase-Y`)

## 33. Phase Yc Final Production-Readiness Closure [COMPLETE]

- **Phase Yc: Final Production-Readiness Closure [COMPLETE]** — Closed the final Claude recovered degraded-delivery contract gap and the final `NotificationSink` boundary bypass reopened by focused production-readiness review, across sprints Y.12–Y.13. (Implementation target: `integrate/phase-Y`)

## 34. Phase Yd Develop-Gate Closure [COMPLETE]

- **Phase Yd: Develop-Gate Closure [COMPLETE]** — Documented and closed the full Phase Y blocker set (recovered Claude logical-message-set, production notification boundary, retained-runtime composition, candidate closure, thin-liveness) across sprints Y.14–Y.18; readiness record at `19376e42` explicitly authorized Phase Y to land on `develop` and Phase Z to begin. (Integration target: `integrate/phase-Y`)

## 35. Phase Ye Daemon Ownership Simplification [COMPLETE]

- **Phase Ye: Daemon Ownership Simplification [COMPLETE]** — Simplified `RuntimeStatusCache`, `NotificationRuntime`, and `ReconcileRuntime` ownership surfaces from lock-heavy to immutable snapshot publication and bounded channel/actor ownership across sprints Y.19–Y.23; closed with `ADR-015` acceptance. (`Phase Ye: closed — Y.23 phase-end proof recorded and ADR-015 accepted.`)

## 36. Phase Z Smoke, Dogfood, And Release Sign-Off [COMPLETE]

- **Phase Z: Smoke, Dogfood, And Release Sign-Off [COMPLETE]** — Validated the first daemon + SQLite mail-SSOT release with real-binary smoke, roster truth cutover, watcher-owned Claude config ingest, boundary lint gates, `atm-dev` canary and dogfood, and final release sign-off; verdict `READY` on `feature/pZ-smoke-atm-graft @ 84935774` authorized in `docs/phase-Z/readiness.md` (`PZ-ATM-GRAFT-QA-3 PASS — PR #365`). (Sprints Z.1–Z.24 and Z.3–Z.4; integration branch: `integrate/phase-Z`)

## 37. Phase AB Windows/macOS Cross-Host Smoke

Status summary:
- `Phase Z` is complete and remains the accepted same-host release-readiness
  line on `develop`.
- Windows same-host build/test and release-binary daemon parity have been
  restored on the post-`Z` baseline.
- cross-host messaging between Windows and macOS has not yet been validated in
  one authoritative executable smoke phase.
- `Phase AB` is the next planning line and is not yet started.

Planning branch:
- `plan/phase-AB`

Future integration branch:
- `integrate/phase-AB`

Goal:
- validate Windows <-> macOS cross-host ATM messaging on real binaries
- keep clean-room disposable state as the first validation lane
- prove durable send/read/ack, degraded notification visibility, and
  retry-visible recovery across hosts
- revalidate on copied state only after the disposable lane passes

Execution shape:
- `AB.1` cross-host harness and clean-room baseline
  - branch: `feature/pAB-s1-cross-host-harness-and-clean-room-baseline`
- `AB.2` one-way cross-host delivery
  - branch: `feature/pAB-s2-one-way-cross-host-delivery`
- `AB.3` cross-host ack round-trip
  - branch: `feature/pAB-s3-cross-host-ack-round-trip`
- `AB.4` degraded notification and retry-visible recovery
  - branch: `feature/pAB-s4-degraded-notification-and-retry-visible-recovery`
- `AB.5` copied-state revalidation and readiness closeout
  - branch: `feature/pAB-s5-copied-state-revalidation-and-readiness-closeout`

Immediate planning outputs:
- `docs/plans/phase-AB/plan-phase-AB.md`
- `docs/plans/phase-AB/cross-host-smoke-checklist.md`
- `docs/plans/phase-AB/cross-host-findings-ledger.md`
- `docs/plans/phase-AB/readiness.md`
- `docs/plans/phase-AB/sprint-AB1.md`
- `docs/plans/phase-AB/sprint-AB2.md`
- `docs/plans/phase-AB/sprint-AB3.md`
- `docs/plans/phase-AB/sprint-AB4.md`
- `docs/plans/phase-AB/sprint-AB5.md`

Acceptance / Phase Entry Gate:
- `Phase Z` must remain closed on `develop`
- the clean-room disposable host-pair lane must pass before copied-state
  validation begins
- the phase does not close until both disposable and copied-state cross-host
  smoke lanes pass with retained evidence

## 38. Chore: ADR Rationale Audit [COMPLETE]

- `CHORE-ADR-AUDIT-001` removed sprint-doc and phase-plan rationale
  dependencies from permanent ADRs, inlined the missing durable rationale in
  the affected records, and kept any surviving sprint references as historical
  execution context only.
  - branch: `chore/docs-restructure`
  - authoritative source: `docs/adr/INDEX.md`
