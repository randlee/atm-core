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
- the canonical daemon API contract is
  [`docs/atm-daemon/http-api.md`](./atm-daemon/http-api.md) and its checked-in
  OpenAPI specification; the legacy frame `protocol-icd.md` was intentionally
  removed
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
  single-message `atm read`, including the historical ATM-authored Claude JSONL
  compatibility envelope for oversized message bodies
- the historical remaining Phase S implementation work continued in:
  - `S.6` daemon post-mortem runtime remediation
  - `S.7` bounded queue-query implementation
  - `S.8` historical Claude JSONL compatibility-envelope implementation
  - `S.9` host-scoped retained logging defaults, including historical
    watcher/reconcile
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

Phase-AG planning note:
- `Phase AG` is the active cross-host validation line that follows the
  completed same-host release-readiness work in `Phase Z`
- the authoritative planning document is
  [`docs/plans/phase-AG/plan-phase-AG.md`](./plans/phase-AG/plan-phase-AG.md)
- `Phase AG` now has two historical sections:
  - the completed early validation attempts on
    `feature/cross-host-communication`
  - the replan/corrective line on
    `plan/phase-ag-multihost-advertise-allowlist`
- early AG execution proved that validation alone could not close the phase:
  the product was missing durable cross-host control-plane surfaces
- the current AG prerequisite product work is:
  - SQLite-backed interface selection/bind configuration
  - SQLite-backed deny-by-default exact-host allowlist enforcement
  - CLI commands to manage both
  - `atm doctor` visibility for both
  - retained loopback self-test support as a supported diagnostic mode
- only after that product work lands does AG return to live Windows/macOS
  host-pair validation and copied-state release proof
- the AG corrective routing/revalidation plan is now merged into `develop`, and
  execution proceeds on separate per-sprint worktrees beginning with
  `feature/pAG-s11-remote-target-contract`
- the remaining ruthless-boundary cleanup and cross-host unification line is
  split into separate critically reviewed hardening sprints `AG.18` through
  `AG.25` on top of the AG.11-AG.17 corrective line
- transport security / encryption remains a later AG sprint concern and must
  not be implied by earlier functional cross-host closure
- standalone follow-up fix work also exists off `develop` for identifier
  hardening: `fix/agent-team-name-charset-validation`, tracked by
  [`docs/plans/sprint-agent-team-charset-hardening.md`](./plans/sprint-agent-team-charset-hardening.md).
  Its scope is to tighten the repo-wide `<agent>` / `<team>` charset contract
  to path-segment-safe, delimiter-safe identifiers and to inject the matching
  centralized validation change through the normal develop-based path.

Phase-AD planning note:
- `Phase AD` is the active release-blocking correction line for caller
  identity ownership, direct post-send emission, and deletion of retired
  Claude/reconcile/notification-runtime paths
- the authoritative planning document is
  [`docs/plans/phase-AD/plan-phase-AD.md`](./plans/phase-AD/plan-phase-AD.md)
- the planning branch is `plan/daemon-graft-boundary-reset`
- the execution integration branch is `integrate/phase-AD`
- the corrective release line extends beyond `AD.11`; `AD.12` through `AD.20`
  are required closure sprints for the graft-boundary reset, ULID-only
  identity cleanup, raw CLI runtime-root unification, and read-path
  consistency repair
- the corrective release line extends again through `AD.25` to `AD.30` for
  post-send closeout and Windows daemon-depth proof
- the corrective release line extends again through `AD.31` to `AD.35` for
  the mailbox peek surface, owner-only mutation reset, durable ack intent,
  self-address/self-ack closure, and final messaging regression closeout

Phase-AE planning note:
- `Phase AE` is the active installed user-documentation planning line on top of
  the accepted `Phase AD` baseline
- the authoritative planning document is
  [`docs/plans/phase-AE/plan-phase-AE.md`](./plans/phase-AE/plan-phase-AE.md)
- the planning branch is `plan/phase-AE`
- the execution integration branch is `integrate/phase-AE`
- `Phase AE` owns the repo-authored `docs/user-documents/` corpus, installed
  delivery under `share/doc/atm/`, concise `atm help` surfacing, fenced
  example and relative-link verification, release freshness gating, and the
  phase-close installed-doc proof artifact

Phase-AF planning note:
- `Phase AF` is the 1.3.1 reliability recovery line following 1.3.0 dogfood
  findings; it does not supersede the retained Phase AE installed-documentation
  scope.
- the authoritative phase plan is [`docs/plans/phase-af/README.md`](./plans/phase-af/README.md)
  with hardened sprint documents for AF-1 host-wide singleton, AF-2
  observability/release gates, and AF-3 native send-input integrity.
- the accepted implementation branch is `integrate/phase-AF`; AF-1, AF-2, and
  AF-3 are merged there at `52c5c338`, with docs-only readiness corrections at
  `d5420b0f`.
- PR #539 is merged to `develop` at `98a4e66c`.
- AF-1 is the release blocker: no 1.3.1 RC or daemon-spawning full smoke may
  proceed until its process-level singleton proof is green.
- `smoke-test/1.3.1-cross-host` is the repo-published cross-host RC evidence
  sprint on top of the accepted AF implementation line. Its authoritative plan
  is
  [`docs/plans/phase-af/smoke-1.3.1-cross-host-plan.md`](./plans/phase-af/smoke-1.3.1-cross-host-plan.md),
  and its Windows handoff checklist is
  [`docs/plans/phase-af/smoke-1.3.1-windows-checklist.md`](./plans/phase-af/smoke-1.3.1-windows-checklist.md).

Prompt-hardening note:
- `feature/prompt-hardening` is the prompt/template hardening branch for
  concise evidence discipline across dev, QA, and review reporting.
- the authoritative plan is
  [`docs/plans/prompt-hardening/plan-prompt-hardening.md`](./plans/prompt-hardening/plan-prompt-hardening.md)

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
- Phase AG is the active planning line for Windows/macOS cross-host ATM
  validation after the accepted Phase Z baseline; Phase AB is historical input
  only.
- Phase AD is the active planning line for release-blocking caller-identity,
  post-send, and retired-subsystem cleanup on top of the accepted `1.2.3`
  baseline.
- Phase AE is the active planning line for installed end-user documentation as
  a shipped release surface.
- Phase AF is the active 1.3.1 reliability recovery line under phase-end
  review on `integrate/phase-AF`; AF-1, AF-2, and AF-3 are merged and the
  remaining closeout work is release-evidence and QA-gate completion.
- the current merged workspace contains:
  - `crates/atm-architecture`
  - `crates/atm-core`
  - `crates/atm`
  - `crates/atm-daemon`
  - `crates/atm-daemon-bootstrap`
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
- `crates/atm-daemon-bootstrap`
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

### Phase AF: 1.3.1 Reliability Recovery [PHASE-END REVIEW]

Status summary:
- Phase AF is the active reliability-recovery line following 1.3.0 dogfood.
- AF-1, AF-2, and AF-3 are merged on `integrate/phase-AF`; PR #539
  is merged to `develop` at `98a4e66c`.
- Accepted implementation branch: `integrate/phase-AF`.
- Integration target: `develop`.
- The authoritative plan is
  [`docs/plans/phase-af/README.md`](./plans/phase-af/README.md).
- The authoritative closure checklist is
  [`docs/plans/phase-af/readiness.md`](./plans/phase-af/readiness.md).

Goal:
- restore the literal one-daemon/one-durable-state-root invariant for an OS
  user on one host
- make post-send configuration, daemon health, errors, capacity, and release
  cutover observable and safe
- preserve native inline, stdin, and file message bytes across the CLI-to-
  daemon boundary

Deliverables:
- AF-1 host-runtime singleton admission, lifecycle, and process-proof design
- AF-2 doctor, connection-worker, capacity/deadline, and compatibility-gate
  design
- AF-3 client-side stdin materialization and release-binary byte-readback
  design

Sprint line:
- `AF-1` `feature/atm-daemon-singleton-hardening`
- `AF-2` `feature/pAF-s2-observability-release-gates`
- `AF-3` `feature/pAF-s3-native-send-input-integrity`

Acceptance:
- Phase AF exit criteria are satisfied only through
  `docs/plans/phase-af/readiness.md` and its linked plan validations.

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
  [exhaustive AC.0 type ledger](./plans/phase-AC/type-ledger.md), and the final
  cross-document consistency corrections for the AC sprint set.
- `AC0-DOCS-MIGRATE-1` (`chore/ac-docs-migrate`) is complete: after merging
  `origin/develop`, the full Phase AC plan set now lives under
  `docs/plans/phase-AC/`; the legacy pre-restructure Phase AC locations are
  gone, and all in-repo references were updated to the new layout.
- The authoritative plan lives in [`docs/plans/phase-AC/`](./plans/phase-AC/).
- The authoritative closure checklist is
  [`docs/plans/phase-AC/readiness.md`](./plans/phase-AC/readiness.md).

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
- `AC.1` `feature/pAC-s1-atm-storage-contract-and-canonical-types` `complete`
- `AC.2` `feature/pAC-s2-atm-storage-claude-extraction` `complete`
- `AC.3` `feature/pAC-s3-sqlite-backend-convergence` `complete`
- `AC.4` `feature/pAC-s4-atm-core-storage-boundary-adoption` `complete`
- `AC.5` `feature/pAC-s5-rpc-envelope-and-domain-type-unification` `complete`
- `AC.6` `feature/pAC-s6-cleanup-and-deletion-closeout` `complete`
- `AC.7` `feature/pAC-s7-sqlserver-readiness-proof` `complete`
- `AC.8` `feature/pAC-s8-thin-client-bootstrap-dependency-relock` `complete`

Completion note:
- `AC.7` proves SQL Server readiness from the real post-`AC.6` contract,
  lands `crates/atm-storage-sqlserver-proof` as a compile-only backend proof,
  and closes the final backend-interchangeability issue without another storage
  reset.

AC.8 follow-on note:
- `AC.8` is the thin-client dependency relock follow-on that removes the
  unconditional `atm-graft -> atm-daemon-bootstrap` compile-time edge while
  preserving the standard same-host daemon auto-start convenience path through
  shared `atm-daemon-client` helpers and machine-readable boundary-policy
  enforcement.

AC.6 closeout:
- deleted the speculative `TaskStore` family from `atm-core` and removed the
  last runtime/daemon compile bridge assumptions instead of preserving them as
  compatibility surface
- removed the old Claude `SourceIngress*` / `ProjectionExport*` shared wrapper
  surface and cut daemon consumers over to direct
  `atm-storage-claude::compat` functions and canonical `SourceFileRecord`
- removed `SqliteObservability*` from `atm-storage` and left that surface owned
  by `atm-storage-rusqlite` as the backend-owned sqlite observability seam used
  during runtime assembly

Acceptance:
- Phase AC exit criteria are satisfied only through
  `docs/plans/phase-AC/readiness.md`

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

- **Phase P: File-I/O Ownership And Single-Write-Path Hardening [COMPLETE]** — Applied one explicit file-I/O ownership model (read_only / read_possible_write / read_modify_write) across every live file family, eliminated ad hoc write paths, completed lock-sentinel gap closure (P.9/P.10), and reconciled requirements/architecture docs with the landed implementation. The temporary workflow sidecar introduced during this phase has since been retired; SQLite is the exclusive mailbox-state authority. (Sprints P.1–P.5, P.6–P.10, M.F1; PRs #111–#115, #120; integrated to `develop`)

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
- repo-tracked dogfood config does not carry live `[[atm.post_send_hooks]]`
  defaults or committed `tmux_pane_id` routing truth
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

## 37. Phase AG Windows/macOS Cross-Host Validation [HISTORICAL]

Status summary:
- `Phase Z` is complete and remains the accepted same-host release-readiness
  line on `develop`.
- Windows same-host build/test and release-binary daemon parity have been
  restored on the post-`Z` baseline.
- early AG validation attempts were executed and produced real findings, but
  they also proved the original validation-only framing was insufficient.
- the missing AG product surfaces are now explicit:
  - durable daemon interface/bind configuration
  - durable inbound exact-host allowlist enforcement
  - CLI management for both
  - `atm doctor` visibility for both
  - retained loopback self-test support
- Phase AG is retired. It documents the rejected custom-frame/TCP design and
  must not be used for implementation or release evidence; Phase AI owns the
  replacement HTTP/UDS and HTTPS proof line.
- `Phase AB` remains historical source material only.

Planning branch:
- historical early execution: `feature/cross-host-communication`
- earlier corrective replan: `plan/phase-ag-multihost-advertise-allowlist`
- current corrective routing/revalidation plan source: `develop`

Branch-routing note:
- PR #542 (`feature/cross-host-communication` -> `develop`) is retained as the
  historical early-AG planning/execution record
- PR #555 (`plan/phase-ag-multihost-advertise-allowlist` -> `develop`) is the
  earlier corrective AG replanning line
- the hardened AG.11 through AG.15 execution line now uses separate sprint
  branches/worktrees:
  `feature/pAG-s11-remote-target-contract`,
  `feature/pAG-s12-localhost-proof`,
  `feature/pAG-s13-selfip-proof`,
  `feature/pAG-s14-integration-coverage`, and
  `feature/pAG-s15-othermac-smoke`
- if AG later opens product-code fixes from concrete findings, those follow-up
  branches must declare their own normal integration path explicitly

Goal:
- preserve what AG.1 / AG.2 / AG.3 already established
- finish the missing product control plane before claiming real closure
- validate Windows <-> macOS cross-host ATM interfaces on real binaries after
  that product surface exists
- prefer the simplest real network path first (plain LAN is acceptable and
  preferable when available, including Mac Studio)
- revalidate on copied state only after the disposable lane passes
- sequence transport security / encryption after functional cross-host
  operability is real

Execution shape:
- `AG.1` cross-host setup contract and channel bring-up
- `AG.2` core cross-host interface validation
- `AG.3` daemon loopback self-test surface
- `AG.4` durable interface configuration and binding
- `AG.5` durable host allowlist enforcement
- `AG.6` doctor visibility for the cross-host control plane
- `AG.7` live cross-host revalidation
- `AG.8` transport security and encryption hardening
- `AG.10` secured cross-host transport implementation
- `AG.9` historical reviewed copied-state verdict for the pre-corrective line
- `AG.11` exact remote-target contract and dispatch routing
- `AG.12` localhost full-function same-host remote-target proof
- `AG.13` self-IP full-function same-host remote-target proof
- `AG.14` automated integration coverage for the corrective path
- `AG.15` other-Mac cross-host smoke for the corrective path
- `AG.16` Windows/macOS cross-host smoke for the corrective path
- `AG.17` corrective copied-state revalidation and final release verdict
- `AG.18` collapse Compose and DirectDeliver into one envelope/handler
- `AG.19` delete separate remote-ack execution path
- `AG.20` move deferred/replay policy out of transport
- `AG.21` collapse duplicate dispatch routing and inbound persistence paths
- `AG.22` relocate host matching and endpoint selection out of transport
- `AG.23` remove synthetic deferred-receipt construction from daemon dispatch
- `AG.24` stop transport from mutating request shape before send
- `AG.25` live two-daemon-pair proof for the unified cross-host line

Immediate planning outputs:
- `docs/plans/phase-AG/plan-phase-AG.md`
- `docs/plans/phase-AG/readiness.md`
- `docs/plans/phase-AG/sprint-AG1.md`
- `docs/plans/phase-AG/sprint-AG2.md`
- `docs/plans/phase-AG/sprint-AG3.md`
- `docs/plans/phase-AG/sprint-AG4.md`
- `docs/plans/phase-AG/sprint-AG5.md`
- `docs/plans/phase-AG/sprint-AG6.md`
- `docs/plans/phase-AG/sprint-AG7.md`
- `docs/plans/phase-AG/sprint-AG8.md`
- `docs/plans/phase-AG/sprint-AG9.md`
- `docs/plans/phase-AG/sprint-AG10.md`

Acceptance / Phase Entry Gate:
- `Phase Z` must remain closed on `develop`
- no speculative code work begins before the first failed validation row exists
- the clean-room disposable host-pair lane must pass before copied-state
  validation begins
- the phase does not close until both disposable and copied-state cross-host
  validation lanes pass with retained evidence or are blocked by named findings

## 38. Phase AD Caller Identity And Post-Send Runtime Simplification [COMPLETE]

Status summary:
- `Phase AD` is complete on `integrate/phase-AD` as the release-blocking
  correction line for the accepted `1.2.3` baseline.
- it restores caller-owned identity handling so the CLI fails closed when
  identity is absent and the daemon never guesses identity
- it narrows post-send behavior back to a direct persist-then-emit seam with
  sender-visible warnings on emission failure
- it deletes retired Claude inbox, reconcile, and notification-runtime paths
  that no longer belong on the accepted line
- `AD.1` (`feature/pAD-s1-caller-identity-ownership-restore`) is complete:
  retained caller-owned CLI commands now resolve caller identity and caller
  team at the CLI boundary, fail closed when either is missing, and carry both
  fields explicitly to daemon-backed request DTOs.
- `AD.2` (`feature/pAD-s2-config-identity-removal-and-doctor-repair`) is
  complete: obsolete config-driven caller identity fallback is retired, doctor
  remains the identity-free diagnostic exception, and the accepted caller
  context contract is reflected in CLI and doctor behavior.
- `AD.3` (`feature/pAD-s3-claude-backend-and-inbox-nudge-retirement`) is
  complete: the retired Claude backend and Claude JSON inbox nudge path are no
  longer part of the accepted runtime line.
- `AD.4` (`feature/pAD-s4-reconcile-runtime-removal`) is complete:
  `ReconcileRuntime` and the watched-source/import runtime lane are removed
  from accepted daemon behavior.
- `AD.5` (`feature/pAD-s5-notification-runtime-removal-and-post-send-detachment`)
  is complete: daemon notification queue/worker delivery was removed and
  post-send warning ownership was detached from the old notification-runtime
  path.
- `AD.6` (`feature/pAD-s6-post-send-nudge-contract-simplification`) is
  complete: post-send ownership is reduced to explicit emitter seams with one
  stable sender-warning contract for emission failure.
- `AD.7` (`feature/pAD-s7-local-tmux-post-send-emitter`) is complete: local
  tmux nudges use authoritative SQLite roster pane metadata instead of repo
  config assumptions.
- `AD.8` (`feature/pAD-s8-graft-post-send-emitter`) is complete: graft-backed
  post-send emission is isolated behind the graft advisory boundary with
  matching governance records and readiness evidence.
- `AD.9` (`feature/pAD-s9-update-member-cli-and-roster-repair-path`) is
  complete: `atm teams update-member` is the accepted repair path for pane and
  member metadata, with `MemberNotFound` aligned to the not-found error family.
- `AD.10` (`feature/pAD-s10-directory-metadata-and-doctor-contract-cleanup`)
  is complete: durable `home_dir`, runtime `live_cwd`, and log-only
  `launch_cwd` terminology and doctor projections are cleaned up and made
  consistent.
- `AD.11` (`feature/pAD-s11-smoke-and-readiness-closeout`) is complete: smoke
  artifacts, readiness validation, and closeout evidence converge on one
  accepted branch tip for the phase release gate.

Planning branch:
- `plan/daemon-graft-boundary-reset`

Integration branch:
- `integrate/phase-AD`

Goal:
- restore CLI-owned caller identity resolution
- restore direct post-send nudge emission after persistence
- remove retired Claude/reconcile/notification-runtime behavior from the
  accepted line
- finish the SQLite-backed roster repair path for pane and member metadata

Deliverables:
- required caller identity on caller-owned CLI -> daemon requests
- direct `PostSendHookEmitter` contract plus boundary-governance records
- local tmux and graft-backed emitter paths with sender-visible warning
  behavior
- deletion of `atm-storage-claude`, `ReconcileRuntime`, and daemon
  notification queue/worker runtime
- `atm teams update-member` as the accepted repair path for existing member
  metadata
- corrective `AD.12` through `AD.22` closure of:
  - ULID-only retained message identity
  - graft advisory boundary reset
  - raw CLI runtime-root unification
  - read-mutation and read-selector output consistency
  - shipped built-in post-send nudge plus bounded template override support
  - pane-routing ownership cleanup out of committed repo config
- follow-up `AD.25` through `AD.30` closure of:
  - explicit built-in template override lifecycle/reset semantics
  - real post-send boundary wiring plus mixed-success hook accounting
  - upstream extraction of built-in template resolution out of the built-in
    delivery path, with any retained `atm internal-nudge` helper reduced to a
    resolved-envelope render/deliver leaf rather than the shipped default
  - deterministic `atm-graft` host-nudge race closure
  - one authoritative Phase AD post-send smoke matrix covering exactly:
    - external hook success
    - external hook partial failure
    - built-in fallback
    - override reset-to-default
    - explicit disable behavior when retained
  - separate Windows daemon integration-depth proof for the remaining local IPC
    shutdown/error/rejection cases
- follow-up `AD.31` through `AD.35` closure of:
  - explicit split between non-mutating `atm peek` inspection and owner-only
    mutating `atm read`
  - owner-only mutation for `send`, `read`, `ack`, and `clear`, with no
    mutating impersonation path
  - durable sender-owned `requires_ack` message state and deletion of
    read-time ack creation
  - self-addressed send rejection and self-ack poison termination
  - operator-protocol/help/regression closeout for the repaired messaging
    model

Sprint line:
- `AD.1 [COMPLETE]` `feature/pAD-s1-caller-identity-ownership-restore`
- `AD.2 [COMPLETE]` `feature/pAD-s2-config-identity-removal-and-doctor-repair`
- `AD.3 [COMPLETE]` `feature/pAD-s3-claude-backend-and-inbox-nudge-retirement`
- `AD.4 [COMPLETE]` `feature/pAD-s4-reconcile-runtime-removal`
- `AD.5 [COMPLETE]` `feature/pAD-s5-notification-runtime-removal-and-post-send-detachment`
- `AD.6 [COMPLETE]` `feature/pAD-s6-post-send-nudge-contract-simplification`
- `AD.7 [COMPLETE]` `feature/pAD-s7-local-tmux-post-send-emitter`
- `AD.8 [COMPLETE]` `feature/pAD-s8-graft-post-send-emitter`
- `AD.9 [COMPLETE]` `feature/pAD-s9-update-member-cli-and-roster-repair-path`
- `AD.10 [COMPLETE]` `feature/pAD-s10-directory-metadata-and-doctor-contract-cleanup`
- `AD.11 [COMPLETE]` `feature/pAD-s11-smoke-and-readiness-closeout`
- `AD.12` `feature/pAD-s12-graft-boundary-reset-planning`
- `AD.13` `feature/pAD-s13-ulid-message-identity-reset`
- `AD.14` `feature/pAD-s14-shared-graft-boundary-surface-reset`
- `AD.15` `feature/pAD-s15-daemon-advisory-runtime-deletion`
- `AD.16` `feature/pAD-s16-thin-graft-receiver-reset`
- `AD.17` `feature/pAD-s17-boundary-reset-verification-closeout`
- `AD.18` `feature/pAD-s18-raw-cli-runtime-root-unification`
- `AD.19` `feature/pAD-s19-read-mutation-output-consistency-repair`
- `AD.20` `feature/pAD-s20-read-body-search-metadata-consistency-repair`
- `AD.21` `feature/pAD-s21-built-in-post-send-nudge-and-template-overrides`
- `AD.22` `feature/pAD-s22-nudge-routing-state-and-dogfood-transition-cleanup`
- `AD.25` `feature/pAD-s25-post-send-hook-emitter-live-wiring`
- `AD.26` `feature/pAD-s26-rule001-observability-seam-closure`
- `AD.27` `feature/pAD-s27-upstream-built-in-template-resolution`
- `AD.28` `feature/pAD-s28-atm-graft-timing-independent`
- `AD.29` `feature/pAD-s29-phase-ad-post-send-smoke-matrix`
- `AD.30` `feature/pAD-s30-windows-daemon-integration-depth`
- `AD.31` `feature/pAD-s31-mailbox-peek-surface-and-owner-only-mutation-reset`
- `AD.32` `feature/pAD-s32-durable-ack-intent-and-read-semantics-reset`
- `AD.33` `feature/pAD-s33-self-addressed-send-rejection`
- `AD.34` `feature/pAD-s34-self-ack-loop-termination-and-historical-poison-cleanup`
- `AD.35` `feature/pAD-s35-messaging-protocol-and-regression-closeout`

Acceptance:
- the phase closes only through
  [`docs/plans/phase-AD/readiness.md`](./plans/phase-AD/readiness.md)
- readiness is valid only if `AD.1` through `AD.11`, `AD.12` through
  `AD.22`, `AD.25` through `AD.30`, and `AD.31` through `AD.35` all pass on
  the accepted line
- `AD.30` is the sole sprint allowed to author the Windows/post-send
  sub-line closeout record in `docs/plans/phase-AD/readiness.md`, while
  `AD.35` is the sole sprint allowed to author the final Phase `AD` messaging
  follow-up verdict after `AD.31` through `AD.35` are complete
- `AD.24` is reserved in the sibling smoke-test planning worktree and is
  consumed by `AD.29`; its harness scope must not be duplicated in the
  follow-up line

## 39. Chore: ADR Rationale Audit [COMPLETE]

- `CHORE-ADR-AUDIT-001` removed sprint-doc and phase-plan rationale
  dependencies from permanent ADRs, inlined the missing durable rationale in
  the affected records, and kept any surviving sprint references as historical
  execution context only.
  - branch: `chore/docs-restructure`
  - authoritative source: `docs/adr/INDEX.md`

## 40. Phase AI — HTTP daemon and minimal cross-host transport [ACTIVE — implementation through AI.38; readiness blocked]

Planning branch: `plan/phase-ai-planning`
Integration branch: `integrate/phase-ai-31-33`

Implementation is merged through AI.38. Post-AI.38 legacy-finding and
hardening cleanup is in progress on follow-up branches. This implementation
status does not close the phase: [`docs/plans/phase-ai/readiness.md`](./plans/phase-ai/readiness.md)
still blocks release pending physical two-Mac and Mac↔Windows peer evidence.

The retained local roster-repair follow-up is planned in
[`docs/plans/teams-remove-member/sprint-02.md`](./plans/teams-remove-member/sprint-02.md).
It adds the narrowly scoped `atm teams remove-member` command on its own
feature branch; it is not cross-host transport work and does not alter the
Phase AI readiness gate.

AI.1 (`feature/pAI-1-daemon-preag-reset`, PR #592) is the reviewed deletion
baseline. It retains only the local-IPC singleton while deleting peer transport,
replay/store support, and retired boundary adapters. It supersedes the abandoned
PR #590 line. AI.2 onward rebuild from that baseline: HTTP over UDS replaces the
custom local frame protocol, and the same router later serves authenticated
HTTPS/TCP peers. The final line has no legacy Windows local-transport fallback, peer/replay state, parallel
send/ack paths, or cross-host-specific mailbox logic.

Implementation Branches:

| Sprint | Status | Branch | Artifacts |
| --- | --- | --- | --- |
| `AI.1` | `complete` | `feature/pAI-1-daemon-preag-reset` | deleted peer transport/replay state and retired daemon compatibility adapters |
| `AI.2` | `complete` | `feature/pAI-s2-storage-topology` | storage topology cleanup, backend-neutral runtime factory, atm-core boundary retirement gate |
| `AI.3` | `complete` | `feature/pAI-s3-error-contract-foundation` | serializable error contract foundation and retired protocol error envelope cleanup |
| `AI.4` | `complete` | `feature/pAI-s4-error-consumer-migration` | consumers migrated onto the two-field error contract |
| `AI.5` | `complete` | `feature/pAI-s5-chat-address-identity` | chat-address identity contract aligned for HTTP daemon ingress |
| `AI.6` | `complete` | `feature/pAI-s6-http-uds-router` | REST router and HTTP-over-UDS local daemon transport, with AI.7 write-graph waiver recorded |
| `AI.7` | `complete` | `feature/pAI-s7-canonical-write-path` | canonical write request, single host-routing seam, and collapsed send/ack ingress |
| `AI.8` | `complete` | `feature/pAI-s8-crosshost-control-plane` | durable HTTPS interface, certificate, and trust configuration |
| `AI.9` | `complete` | `feature/pAI-s9-https-peer-transport` | peer HTTPS transport |
| `AI.10` | `complete` | `feature/pAI-s10-crosshost-proof-closeout` | proof matrix and closeout; live physical-peer rows remain readiness blockers |
| `AI.11` | `complete` | `feature/pAI-s11-post-merge-remediation` | route-specific HTTP bodies and Windows loopback-TCP local transport |
| `AI.12` | `complete` | `feature/pAI-s12-post-write-router` | canonical post-write peer routing and immutable outbound persistence |
| `AI.13` | `complete` | `feature/pAI-s13-peer-smoke-contract` | repository-owned peer-pair smoke runner and release evidence contract |
| `AI.14` | `complete` | `feature/pAI-s14-mac-peer-smoke` | physical Mac↔Mac peer-pair proof implementation; live evidence remains blocked |
| `AI.15` | `complete` | `feature/pAI-s15-windows-peer-smoke` | physical Mac↔Windows peer-pair proof implementation; live evidence remains blocked |
| `AI.16` | `complete` | `feature/pAI-s16-offline-reconciliation` | durable-age-bounded canonical-message reconciliation |
| `AI.17` | `complete` | `feature/pAI-s17-hermes-chat-identity` | ambient `ATM_CHAT_ID` identity context |
| `AI.18` | `complete` | `feature/pAI-s18-graft-python-bindings` | PyO3/Maturin graft client/nudge binding |
| `AI.19` | `complete` | `feature/pAI-s19-hermes-graft-integration` | typed Hermes graft bridge after canonical persistence |
| `AI.20` | `complete` | `feature/pAI-s20-hermes-bridge-deployment` | per-profile launchd bridge deployment and runbook |
| `AI.21` | `complete` | `feature/pAI-s21-hermes-closure` | retained Hermes end-to-end production evidence |
| `AI.21-pre` | `complete` | `feature/pAI-s21pre-crosshost-evidence-harness` | supported peer-smoke harness and plaintext-test diagnostic profile |
| `AI.22` | `complete` | `feature/pAI-s22-loopback-self-send-exemption` | host-qualified self-send exemption and advertised-IP proof path |
| `AI.23` | `complete` | `feature/pAI-s23-crosshost-shared-write-path` | one shared HTTP write path and post-write router |
| `AI.24` | `complete` | `feature/pAI-s24-host-qualified-ack-receipt` | host-qualified ACK receipt and peer nudge |
| `AI.25` | `complete` | `feature/pAI-s25-peer-authority-resolution` | hostname/pin peer authority and live trust refresh |
| `AI.26` | `complete` | `feature/pAI-s26-peer-write-deadline` | propagated peer-write deadline |
| `AI.27` | `complete` | `feature/pAI-s27-peer-delivery-observability` | truthful peer delivery outcomes and terminal events |
| `AI.28` | `complete` | `feature/pAI-s28-bounded-peer-recovery` | bounded recovery after connectivity loss |
| `AI.29` | `complete` | `feature/pAI-s29-crosshost-smoke-rerun` | receiver-proven physical smoke implementation; live evidence remains blocked |
| `AI.30` | `complete` | `feature/pAI-s30-semver-http-compatibility` | schema/HTTP compatibility admission and SemVer prerelease distribution |
| `AI.31` | `complete` | `feature/pAI-s31-async-local-admission` | SQLite-only local admission response; host-qualified peer work signalled after response |
| `AI.32` | `complete` | `feature/pAI-s32-independent-peer-jobs` | bounded non-durable per-ULID peer jobs |
| `AI.33` | `abandoned/superseded` | `feature/pAI-s33-admission-capacity-smoke` | PR #695 closed, not merged; real M5 admission-capacity evidence retained a blocking HTTP 503 throughput failure despite green CI; AI.40 is the active owner of a clean benchmark runner/evidence path |
| `AI.34` | `complete` | `fix/hermes-nudge-endpoint-mismatch` | canonical roster workspace-root resolution for graft nudge endpoint delivery |
| `AI.35` | `complete` | `feature/pAI-s35-graft-root-fallback-observability` | graft-root fallback observability and operator runbook closure |
| `AI.36` | `complete` | `feature/pAI-s36-graft-receiver-ownership` | lease-safe receiver ownership per canonical graft root/team/agent |
| `AI.37` | `complete` | `feature/pAI-s37-hermes-recovery-summary` | ten-second durable-mail-derived recovery summary |
| `AI.38` | `complete` | `feature/pAI-s38-hermes-steer-nudge-delivery` | live and recovery graft wake-ups via non-interrupting steer |
| `AI.39` | `complete` | `feature/pAI-s39-buffered-local-http-framing` | bounded buffered local HTTP request framing |
| `AI.40` | `in_progress` | `feature/pAI-s40-local-transport-benchmark` | clean local transport throughput benchmark; not an extension of abandoned AI.33 script |
| `AI.43` | `complete` | `feature/pAI-s43-remote-https-response-framing` | buffered remote HTTPS response framing |
| `AI.46` | `complete` | `feature/pAI-s46-reports-index` | generated durable reports index |
| `AI.47` | `complete` | `feature/pAI-s47-pages-site-home` | GitHub Pages site home and deployment |
| `AI.48` | `complete` | `feature/pAI-s48-fuzz-tooling-port` | ported `just fuzz` coordinator/probe tooling |
| `AI.49` | `complete` | `feature/pAI-s49-benchmark-report` | durable benchmark JSON and aggregate HTML report |
| `AI.50` | `complete` | `feature/pAI-s50-fuzz-report` | sc-compose-template fuzz report renderer |
| `AI.51` | `complete` | `feature/pAI-s51-local-http-framing-adversarial-campaign` | bounded local HTTP framing campaign |
| `AI.52` | `complete` | `feature/pAI-s52-windows-transport-benchmark` | cwin Windows TCP confirmation after accepted M5 performance evidence |
| `AI3152-TOOLING` | `complete` | `feature/daemon-devcert-signing` | retired self-signed macOS development-signing hook for local daemon builds |

Authoritative plan: [Phase AI plan](./plans/phase-ai/plan-phase-ai.md).

AI.3 (`feature/pAI-s3-error-contract-foundation`) completes the two-field
serializable error contract and removes the retired protocol error envelope.

## 41. Phase AK — Direct peer HTTP delivery [ABANDONED]

Status summary:
- Phase AK is abandoned. It was the planned simplification line for replacing
  the Phase AI peer worker and custom TLS sender with one direct HTTP delivery
  function; Phase AL/AM (the Tokio migration, `atm-http-runtime`) supersedes
  it with a single Tokio-based transport replacement instead of an
  incremental direct-HTTP-sender line.
- `AK.1`–`AK.10` reached implementation completion and merged to
  `integrate/phase-ak` before the line was abandoned; no further AK work is
  dispatched.
- `AK.11`–`AK.17` (the post-AK.10 mandate-correction line) do not proceed.
  `AK.11`'s receiver-hook design is the sole salvaged artifact: AL.1 sources
  it as `archived_reference_source` commit `88bca9d5e232006339f43a4e97eef335531b8a8f`
  (hook-boundary file set and tests only, no wholesale cherry-pick), per
  [Phase AL plan](./plans/phase-al/plan-phase-al.md#baseline-and-entry-gate).
  This does not revive, complete, or re-authorize any other AK code, peer
  transport, replay, listener, or scheduler.
- Planning branch (historical): `plan/mvp-simplification`.
- Integration branch (historical): `integrate/phase-ak`.
- The historical plan is
  [Phase AK plan](./plans/phase-ak/plan-phase-ak.md); its AK.11+ references
  are non-authoritative per that document's own AK.11+ authority notice.

Goal:
- preserve immutable local admission and the one ordinary inbound
  persistence/nudge path while removing peer worker, per-message-thread,
  broad-scan, DNS-thread, and native custom-TLS delivery complexity
- prove direct configured-host HTTP delivery before adding the small optional
  resend cache

Deliverables:
- direct host-alias normalization, one direct no-retry HTTP sender, optional
  timer-driven resend cache, and isolated curl-mTLS provisioning evidence
- deletion of obsolete worker/replay/TLS transport state with governed
  boundary-record updates

Sprint line:
- `AK.1` `feature/pak-s1-crosshost-ack-provenance-recovery`
- `AK.2` `feature/pak-s2-delete-peer-worker`
- `AK.3` `feature/pak-s3-canonical-peer-aliases`
- `AK.4` `feature/pak-s4-direct-peer-http-no-retry`
- `AK.5` `feature/pak-s5-direct-peer-timer-state`
- `AK.6` `feature/pak-s6-remove-legacy-peer-transport`

Acceptance:
- Phase AK acceptance is defined by the authoritative plan's sprint
  validations and its required bidirectional production send/read/ACK/nudge
  proof on the accepted `integrate/phase-ak` line.

## 42. Phase AJ — Runtime observation [IMPLEMENTATION COMPLETE — FINAL QA GATE OPEN]

Phase AJ plans and reviews against `integrate/phase-ai-31-33 @
150391ecdf2e003185bff7d78427cd21509a7981`, the HTTP local transport line for
UDS and TCP. Phase AI merged to `develop`; team-lead recorded the post-merge
SHA, cut `integrate/phase-AJ` from it, reconciled every AJ exact target against
the pinned planning baseline, and revalidated drift before AJ.1 started. A
pre-merge plan finding cites the pinned baseline; a post-merge reconciliation
finding cites both SHAs and the changed target.

All AJ implementation heads, closeout validation, and parent PR merges
(`AJ.1`–`AJ.10`, PRs #735–#745, plus merge-content-recovery PR #758) are
complete. Phase AJ is not closed: a final holistic QA gate finding (a
transport-trust-boundary gap in heartbeat ingress) must be remediated and
reverified before its final status changes.

AJ keeps roster runtime observation in daemon memory: successful
environment-attested CLI/graft activity and heartbeat converge on one current
entry. Session, pid, and state are diagnostic telemetry, not inputs to routing,
nudge, retry, admission, delivery, notification, or policy.

| Sprint | Status | Branch | Purpose |
| --- | --- | --- | --- |
| `AJ.1` | `implementation complete` | `feature/pAJ-s1-session-id-and-protocol` | canonical `SessionId` and additive heartbeat fields |
| `AJ.2` | `implementation complete` | `feature/pAJ-s2-caller-context-env` | environment-attested observation resolver |
| `AJ.3` | `implementation complete` | `feature/pAJ-s3-cli-wire-payload` | transient local CLI/graft request metadata |
| `AJ.4` | `implementation complete` | `feature/pAJ-s4-daemon-cache-touch` | shared daemon cache merge after successful local dispatch |
| `AJ.5` | `implementation complete` | `feature/pAJ-s5-heartbeat-session` | heartbeat session observation convergence |
| `AJ.6` | `implementation complete` | `feature/pAJ-s6-runtime-observation-snapshot` | runtime snapshot and roster projection |
| `AJ.7` | `implementation complete` | `feature/pAJ-s7-runtime-observation-source-guard` | non-authoritative source-use guard |
| `AJ.8` | `implementation complete` | `feature/pAJ-s8-runtime-observation-boundary-record` | machine and human daemon boundary record |
| `AJ.9` | `implementation complete` | `feature/pAJ-s9-runtime-observation-contract-reconciliation` | requirements, ADR, architecture, and team-state reconciliation |
| `AJ.10` | `implementation complete` | `feature/pAJ-s10-runtime-observation-phase-closeout` | evidence-backed phase and status closeout (final QA gate open) |

Each AJ successor begins immediately when its parent's development head is
merged forward into it; do not wait for parent QA approval. Merge the current
parent branch into the child before every child dev/fix round. A child PR may
not complete or merge its target before its parent PR merges.

## 43. Phase AL — Build the Minimal Tokio HTTP Runtime [COMPLETE]

Status summary:
- Phase AL replaces ATM's hand-written synchronous HTTP framing and
  transport-specific request processing with one small `atm-http-runtime`
  library built on Tokio and maintained HTTP/TLS libraries, providing the
  same typed application contract to all clients and all listeners.
- AL is additive: it does not preserve the legacy transport as a
  compatibility architecture and does not add resend/replay. Phase AM
  deletes the legacy implementation once AL proves the replacement.
- Planning branch: `plan/tokio-migration`.
- Baseline: `develop @ 67401907039f92e58e883273f02372a637202f70` (includes
  the completed Phase AJ merge).
- Entry gate: AL.1 starts from that `develop` baseline; it does not require
  Phase AK completion, merge, or revival. AL.1 sources only the approved
  receiver-hook design from archived AK.11 commit `88bca9d5` (see Phase AK
  status above).
- Binding boundary rules:
  [`phase-al-am-runtime-boundary-checklist.md`](./plans/phase-al-am-runtime-boundary-checklist.md).
  Every AL PR must pass them before merging forward.
- The authoritative plan is
  [Phase AL plan](./plans/phase-al/plan-phase-al.md).

Sprint line:
- `AL.1 [COMPLETE]` `sprint-AL1-runtime-contract.md` — runtime contract and archived-hook
  transplant
- `AL.2 [COMPLETE]` `sprint-AL2-canonical-handler.md` — canonical handler
- `AL.3 [COMPLETE]` `sprint-AL3-received-hook.md` — received hook wiring
- `AL.4` `sprint-AL4-shared-client.md` — shared client
- `AL.5 [COMPLETE]` `sprint-AL5-unix-uds.md` — Unix UDS listener
- `AL.6` `sprint-AL6-loopback-tcp.md` — loopback TCP listener
- `AL.7 [ABANDONED]` `sprint-AL7-peer-tls-m5-proof.md` — mTLS peer adapter
  removed from the Phase AL MVP before implementation; retained TLS material
  stays quarantined reference only
- `AL.8` `sprint-AL8-daemon-composition-proof.md` — daemon composition and
  static boundary proof
- `AL.9` `sprint-AL9-physical-proof-ledger-freeze.md` — physical adapter
  matrix, benchmark, cutover/abort, AM ledger freeze
- `AL.10 [ABANDONED]` — proposed M4 hardware-smoke work was superseded before
  a sprint record was accepted; its useful evidence moved to the direct M5 and
  cwin tracks below
- `AL.11 [SUPERSEDED]` — historical M5 hardware-smoke dispatch, replaced by
  the pinned-candidate, direct-peer AL.13 plan
- `AL.12 [SUPERSEDED]` — historical cwin hardware-smoke dispatch, replaced by
  the direct public-CLI AL.14 plan
- `AL.13 [COMPLETE]` `sprint-AL13-m5-direct-crosshost-smoke.md` — M5↔M4
  direct-peer smoke and benchmark evidence
- `AL.14 [BLOCKED]` `sprint-AL14-cwin-direct-crosshost-smoke.md` — cwin
  local smoke and benchmark evidence retained; its Windows-originated
  direct-peer row is infrastructure-blocked
- `AL.15 [BLOCKED]` `sprint-AL15-direct-crosshost-evidence-closeout.md` —
  coordinator closeout remains blocked until AL.9's final physical-proof rows
  are rerun and accepted at one frozen candidate
- `AL.16` `sprint-AL16-hermes-graft-live-proof.md` — installable generic
  `atm-graft` and Hermes-facing `hermes-atm` package boundary; package-side
  candidate is under review, while portable live proof is blocked on a
  reviewed, immutable, deployed Hermes host contract
- `AL.17` `sprint-AL17-hermes-gateway-lifecycle.md` — reviewed, immutable,
  deployed Hermes runner lifecycle/injection contract for the queue MVP;
  required before a portable live package claim
- `AL.18` `sprint-AL18-hermes-telegram-live-proof.md` — installed-package M4
  idle and same-session busy queue proof after AL.17 is deployed
- `AL.19` `sprint-AL19-hermes-m5-py311-verify.md` — M5 multi-interpreter
  package verification; CPython 3.11 is an early wheel-compatibility lane and
  the active M5 Hermes-service lane must be inventoried and proven separately

Acceptance:
- Phase AL acceptance is defined by the authoritative plan's sprint
  validations and the runtime boundary checklist's required evidence set.

## 44. Phase AM — Deletion-Only Transport Cleanup [COMPLETE]

Status summary:
- Phase AM is deletion-only: it removes the legacy transport machinery made
  redundant by `atm-http-runtime` (raw HTTP framing, legacy local/peer
  transport workers, peer-only ingress, resend/replay machinery) without
  preserving, repairing, or extending it.
- Planning branch: `plan/tokio-migration`.
- Baseline: `develop @ 67401907039f92e58e883273f02372a637202f70` plus
  accepted Phase AL.
- Entry gate: AM implementation begins only after AL.9 proves the new
  runtime is the live local and cross-host path. AM may inventory and write
  static guards in parallel with AL but must not delete a live path before
  that proof.
- Binding boundary and transition rules:
  [`phase-al-am-runtime-boundary-checklist.md`](./plans/phase-al-am-runtime-boundary-checklist.md),
  [`phase-al-am-boundary-transition.md`](./plans/phase-al-am-boundary-transition.md).
- The authoritative plan is
  [Phase AM plan](./plans/phase-am/plan-phase-am.md).

Sprint line:
- `AM.1` `sprint-AM1-removal-ledger.md` — deletion ledger, topological
  deletion order, negative architecture guards
- `AM.2` `sprint-AM2-delete-legacy-http.md` — migrate retained non-write
  compatibility callers, then delete legacy HTTP framing
- `AM.3` `sprint-AM3-delete-legacy-local.md` — delete legacy local transport
- `AM.4` `sprint-AM4-delete-legacy-peer.md` — delete legacy peer transport
- `AM.5` `sprint-AM5-delete-replay.md` — delete resend/replay machinery
- `AM.6` `sprint-AM6-minimality-proof.md` — minimality proof

Acceptance:
- Phase AM acceptance is defined by the authoritative plan's sprint
  validations: every production legacy reference has one ledger row or is
  proven dead, no guard is merged early, and the minimality proof confirms
  no compatibility shim survives.

## 45. Phase AN — Decomposed Template Messages And Query Surface [AN.1–AN.15 COMPLETE]

- **Phase AN: Decomposed Template Messages And Query Surface [AN.1–AN.10 COMPLETE]** —
  Added a bounded `sc-composer` adapter boundary, durable template catalog and
  decomposed-message records, render-on-read, FTS search, public
  introspection/query surfaces, and compose guidance. AN.8 closes the original
  decomposed-template phase
  with Q1–Q4 read-only query fixtures, a template-agnostic vocabulary proof,
  and the Tokio HTTP four-cell routing matrix on the Linux/macOS/Windows CI
  lanes. Physical cross-host template synchronization remains intentionally
  out of scope. (Authoritative plan:
  [`plan-phase-an.md`](./plans/phase-an/plan-phase-an.md); evidence:
  [`validation-evidence.md`](./plans/phase-an/validation-evidence.md))

- **AN workflow-metadata extension [COMPLETE]** — adds optional
  template-declared workflow facts, immutable admission snapshots of template
  and instance tag provenance, generic local lifecycle analytics, and an
  opt-in OpenTelemetry-compatible projection. It retains no ATM-specific
  workflow vocabulary and is governed by
  [`ADR-046`](./adr/ADR-046-template-declared-workflow-metadata.md).

Sprint line:
- `AN.9` `feature/pan-s9-template-workflow-contract`
- `AN.10` `feature/pan-s10-template-workflow-admission`
- `AN.11` `feature/pan-s11-workflow-analytics-projection` — local workflow
  lifecycle analytics, query projection, and opt-in telemetry seam
- `AN.12` `feature/an12-workflow-validation-evidence` — retained two-vocabulary
  local validation for admission/provenance, CLI/HTTP/Python query, routing,
  migration compatibility, and best-effort telemetry isolation
- `AN.13` `feature/an13-sc-composer-141-upgrade` — durable output-format
  catalog identity and exact released `sc-composer`/`sc-sha` 1.4.1 adapter pin
- `AN.14` `feature/an14-sc-compose-141-checked-emission` — adapter-only
  checked emission that rejects malformed JSON before send or render-on-read

- **AN.13–AN.15 checked-render upgrade and assurance [COMPLETE]** — AN.13 establishes
  durable adapter-derived output-format identity and adopts the released
  exact `sc-sha`/`sc-composer` 1.4.1 dependency chain in its adapter; AN.14
  makes the
  `atm-template-sc-compose` adapter refuse malformed rendered JSON before
  sending, caching, or render-on-read output. AN.15 then runs a bounded,
  deterministic adversarial campaign over the checked template/catalog
  lifecycle, including captured-environment and immutable-revision oracles.
  The AN.15 HTTP-seam addendum (PR #887,
  `feature/an15-http-fuzz-campaign`) is merged and exercises the current Tokio/Axum
  `atm-http-runtime` boundary with a separately retained, commit-pinned
  four-worker campaign; it is not duplicate pre-Tokio AI.51 work against the
  removed `api::http_frame_reader` module. AN.15 does not make ATM a
  template-approval or lineage-policy engine. The three
  sprints required crates.io to publish `sc-sha`,
  `sc-composer`, and `sc-compose` 1.4.1; published `sc-composer` exports
  `check_rendered_output`, `CheckedOutput`, and `OutputFormat`; and
  [sc-compose #448](https://github.com/randlee/sc-compose/issues/448) supplies
  the direct-library checked-emission regression coverage.
  [AN.13 retained release evidence](./plans/phase-an/sc-compose-141-evidence.md)
  records the satisfied gate; the authoritative scope and closure gates are in
  [`sprint-AN13-sc-compose-141-checked-render.md`](./plans/phase-an/sprint-AN13-sc-compose-141-checked-render.md),
  [`sprint-AN14-sc-compose-141-checked-emission.md`](./plans/phase-an/sprint-AN14-sc-compose-141-checked-emission.md),
  and [`sprint-AN15-adversarial-fuzzing.md`](./plans/phase-an/sprint-AN15-adversarial-fuzzing.md).

## 46. Phase AO — Optional mTLS for the Canonical HTTP Peer Path [COMPLETE — DELIVERED VIA PHASE AO2]

Phase AO replans opt-in mTLS for the active Tokio/Axum peer HTTP path.  The
normal plaintext direct-peer listener and client remain the compatibility
pipeline and must be structurally unchanged when TLS is not selected.  An
explicit runtime mode selects plaintext or mTLS in one shipped daemon build;
mTLS is fail-closed, requiring exact hostname/SNI, certificate pin, trusted
client certificate, and enabled interface configuration, and it never falls
back to plaintext.  Both modes retain one canonical HTTP request, storage,
acknowledgement, and nudge path.  The existing TLS interop crate remains
quarantined fixture/reference material; production runtime code must not
depend on it.

Implementation begins only after the accepted Tokio/Axum runtime line is
active. Phase AM's explicit AO TLS exception preserves the existing TLS helper
boundary while AO is decided; it is not an additional AO entry gate.  AO work
integrates through `integrate/phase-ao2`; the earlier AO plan is retained only
as [archived reference](./archive/phase-ao/).  The authoritative plan is
[Phase AO plan](./plans/phase-ao/plan-phase-ao.md).

Sprint line (all four merged into `integrate/phase-ao2` 2026-08-20 under
AO2.1–AO2.4 titles; landed on `develop` with PR #966 on 2026-08-26):
- `AO.1` `feature/pao-s1-peer-wire-policy` [COMPLETE — PR #961] — ADR-047,
  typed `PeerWireMode` (mTLS default), peer-wire error/recovery contracts,
  boundary records, and executable guards proving the plaintext arm stays on
  the existing direct-peer canonical HTTP pipeline
- `AO.2` `feature/pao-s2-isolated-mtls-stream-adapter` [COMPLETE — PR #965] —
  bounded `peer-tls` adapter with positive and negative stream evidence
- `AO.3` `feature/pao-s3-runtime-peer-wire-mode` [COMPLETE — PR #967] — one
  daemon build selects the original plaintext or mTLS stream establishment
  without application drift
- `AO.4` `feature/pao-s4-peer-wire-proof` [COMPLETE — PR #968] —
  shipped-daemon plaintext/mTLS proof and compatible-baseline performance
  evidence

## 47. Phase AP — Outbound-Only Corporate Network Peer Connectivity [PROPOSED — ENTRY GATE UNBLOCKED]

Phase AP investigates support for a firewalled daemon that may initiate an
outbound connection but cannot accept unsolicited peer TCP. The preferred
direction is an mTLS-authenticated HTTP/1.1 SSE session from the restricted
host to a reachable peer plus ordinary authenticated POST for correlated
responses. It remains online-only: no outbox, retry/replay, or durable relay
is introduced.

AP.1 is mandatory and must execute first on the actual CWin, M4, and M5
machines. It proves—or records a block for—the real outbound DNS/TLS/SSE/POST
path without SSH tunneling, localhost simulation, raw-IP substitution, or a
third-party relay. No AP product implementation begins if that physical proof
does not pass. The authoritative outline is
[Phase AP plan](./plans/phase-ap/plan-phase-ap.md).

Status 2026-08-26: AP's precondition (Phase AO's mTLS runtime line active) is
satisfied — AO merged to `develop` via the AO2 integration (PR #966). AP.1's
physical hardware proof remains the mandatory entry gate; no AP dispatch has
occurred.

## 48. Phase AQ — ATM Send-To Shell Integration [COMPLETE ON DEVELOP (PR #1079) — 5 open follow-ups, 2 tied to Must PRD requirements (R1 GUI E2E, R15 m5); see qa-evidence-master.json follow_ups]

Phase AQ delivers PRD Phase 1 of ATM "Send To": one gesture from the OS file
manager (Finder / Explorer / Nautilus) to a delivered message whose text
names files landed under the recipient host's `$ATM_TEMP`. Cross-host bytes
move via user-configured per-host transfer scripts (sftp default over fleet
SSH; unconfigured hosts fail closed with a setup-doc error) — no envelope
change, no daemon transfer machinery. ADR-055 defines the system-level
`ATM_TEMP` contract (mandatory env var, 30-day TTL sweep) and the
transfer-script seam; thin per-OS shell glue drives the pipeline
`atm teams --json --members | <picker> | atm send --attach "$@" --from-json`.
Phase 1 also delivers `atm queue` — `atm send` with the nudge deferred until
the recipient harness is ready (nudge taxonomy: steer and queue are message
*kinds*, not delivery timings — the physical mechanism may itself defer a
steer-kind notification until the next bare-CLI Stop pull, and mechanism
timing never changes the kind; ADR-054, AQ2.5 addendum) — via a
`nudge_pending_at` marker, a `PendingNudgeStore`
storage capability, graft dual-channel wiring (harness owns landing; Hermes
`/steer`+`/queue` complete), and a tmux idle-drain, under the ADR-054 nudge
taxonomy (nudge = umbrella; steer/queue = kinds) with its code-rename
inventory. Queue ships first; trait foundation first, Herdr second
(reordered 2026-08-26 per Rand). Fourteen sprints: AQ1 trait foundation +
`atm queue` CLI verb + ADR-054 taxonomy + `PendingNudgeStore`; AQ2.6/AQ2.7
Herdr local-steer backend + lifecycle-gated queue wake (most urgent);
[AQ2.6 sprint](./plans/phase-aq/sprint-AQ2-6-herdr-steer-backend.md) is the
authoritative implementation entry for the local Herdr backend and is
complete after the AQ2.6 QA1 fix cycle. The [AQ2.7 sprint](./plans/phase-aq/sprint-AQ2-7-herdr-queue-wake.md)
is complete with the fixed-cadence Herdr queue-wake pump and runtime health
poll projection;
AQ1.5–AQ1.9 graft push-registration (ADR-056), parallel with Herdr; AQ2
graft dual-channel; AQ2.5 queue delivery triggers (heartbeat hooks,
bare-CLI FIFO); AQ3 tmux idle-drain + recovery sweep; AQ4 Send-To core
(ATM_TEMP ADR-055, CLI surface, transfer scripts, sweeper); AQ5 surface +
phase evidence; AQ6 sc-ecosystem dependency preflight (pin-latest
Wyvern/sc-compose/sc-observability + integration tests) + Wyvern
contract-test issue. Plan status (2026-08-26): the earlier plan-QA PASS was
retracted after a whole-tree critical review (FAIL, 10 blocking); all
findings were closed in a finalization pass and the re-entry critical review
PASSed on round 3 (`ea990a8dd`); quality-mgr gate on PR #1019 in progress.
ADR-058 (Herdr local steer backend contract) is the phase's fourth ADR. Branches `feature/aq-N-<slug>` off
`integrate/phase-aq`, all PRs target `integrate/phase-aq`. The authoritative
plan is [Phase AQ plan](./plans/phase-aq/plan-phase-aq.md); source PRD is
[prd-atm-send-to](./plans/phase-aq/prd-atm-send-to.md). PRD Phase 2
(agent-assisted drafting, Wyvern chat sessions) is explicitly deferred.

Phase AQ complete on `develop` (14/14 sprints) — `integrate/phase-aq` →
`develop` merged via PR #1079 (`1f5666fa5`).

Status 2026-08-28: all 14 of the phase's sprints are merged to
`integrate/phase-aq`:

- AQ1 trait foundation + `atm queue` CLI verb/taxonomy — PR #1040
  (`feature/aq-1-trait-foundation`)
- AQ1.5 graft push-registration API — PR #1045
- AQ1.6 graft receiver registration client — PR #1046
- AQ1.7 graft endpoint consumer cutover — PR #1048
- AQ1.8 graft file-record retirement — PR #1049
- AQ1.9 hermes-atm wheel verification + restart-matrix crash guard — PR #1050,
  PR #1070 (`26fb5bc4d`); the m5 live matrix follow-up remains pending
- AQ2 graft dual-channel queue delivery — PR #1051
- AQ2.5 queue delivery triggers (heartbeat CLI surface, bare-CLI FIFO,
  `QueuePull` classifier, ADR-054 delivery-trigger addendum) — PR #1053
- AQ2.6 Herdr local-steer backend — PR #1042
- AQ2.7 Herdr poll-gated queue wake — PR #1056, merged `6c70f88ce`
  (cycle-5 QA-5 PASS, closing commit `d25b049d7`)
- AQ3 tmux idle-drain + recovery sweep — PR #1054, merged `deed32e93`
  (QA-final6 PASS 14/14)
- AQ4 Send-To core (ATM_TEMP, CLI surface, transfer scripts, sweeper) —
  PR #1055, merged `0adce24d6` (QA-3 PASS-with-deferral, 7/9 AC; Windows
  loopback and tailscale legs deferred by ruling)
- AQ5 Send-To surface + phase evidence — PR #1059, merged `53921169e` on
  2026-08-28
- AQ6 sc-ecosystem dependency preflight + Wyvern contract issue — PR #1066,
  final head `29ac4a7c58796f446f3cdd6725f265ea6db2a66a`, merged `edb1a5381`
  on 2026-08-28

Tracked follow-ups (owner in parentheses): AQ1.9-m5 — live m5 restart matrix
and hermes-atm suite run (Phase AQ closeout/AQ6); AQ4-tailscale-m5 — live
tailscale transfer transcript (Phase AQ closeout); AQ4-windows-loopback —
real Windows-host/POSIX-receiver reproduction of the ssh-under-pwsh loopback
gap (Phase AQ closeout); AQ5-gui-e2e — live Finder/Explorer/Nautilus
member-picker GUI E2E transcripts (Rand); AQ6-wyvern-pin-bump — Wyvern pin
behind the latest upstream release, surfaced by ecosystem-preflight on
PR #1076, bump before the next release, not a phase-merge blocker (Phase AQ
closeout). See [Phase AQ plan](./plans/phase-aq/plan-phase-aq.md) for the
authoritative sprint-by-sprint detail and
`docs/plans/phase-aq/.audit/qa-evidence-master.json` for QA/merge
provenance.

## 49. Phase AO2 — Benchmark Safety, Evidence, And Transport Performance [COMPLETE — MERGED TO DEVELOP]

Phase AO2 made physical admission benchmarks safe and repeatable, restored
the bounded writer transaction-coalescing path, and established the benchmark
data, rendering, history, and operator-workflow contracts. It retains the
Tokio/Axum `atm-http-runtime` as the sole daemon path: snapshot/restore,
reporting, and benchmark tooling do not create or preserve a legacy transport
path.

The accepted implementation line includes dedicated benchmark-account
snapshot/restore safety (AO2.5/AO2.5.4), the typed temporary daemon-switch
overlay (AO2.5.3b), writer batching (AO2.6), the four-target benchmark matrix
(AO2.7), versioned JSON/report contracts and historical migration
(AO2.10–AO2.13), peer connection pooling (AO2.14), and the headless official
benchmark trigger (AO2.15). AO2.8 is explicitly descoped; it is not evidence
of Windows coverage.

The canonical operator procedure is the repository
[`benchmark-run` skill](../.claude/skills/benchmark-run/SKILL.md). The phase is
not release-closed until its proof matrix and accepted-line evidence are
recorded in [Phase AO2 readiness](./plans/phase-ao2/readiness.md), including
the required four-target macOS campaign and the separately required Windows
TCP/TLS evidence.

Status 2026-08-26: `integrate/phase-ao2` merged to `develop` via PR #966
(merge commit `9923ef6cb`). Known deferred debt carried out of the phase
gate: 22 pre-existing sc-boundary findings, waived for the AO2 gate as
triage record `AO2-SCBOUNDARY-DEBT-001` and tracked in GH issue #1028 —
these make `just validate` (and CI) red on `develop` until Phase AU retires
them. Post-merge follow-ups (post-mortem finalization, deferred findings
QA-AO215-I003 / AO2-REPORT-F001) are tracked in the readiness doc and
triage records, not here.

## 50. Phase AR — codex-atm Host Integration [PLANNING — REQUIREMENTS DRAFT, OFF-INTEGRATION BRANCH]

Phase AR plans atm-core's side of embedding native agent-team-mail into the
codex CLI fork (`randlee/codex-atm`) — the second production host after
hermes-agent and the first pure-Rust, crates.io-consuming one. The draft
covers seven requirement areas: a publishable `atm-graft` crate (R1), a
Rust-native two-channel (steer + queue) host embedding API with a durable,
bounded, backpressured queue channel (R2), fail-loud activation via
`ATM_IDENTITY`/`ATM_TEAM` replacing the `.atm.toml` silent no-op (R3, issue
#900), a required protocol-sequencing ruling — file-rendezvous graft vs.
daemon long-poll session protocol (R4, issue #899, the blocking
architectural decision), packaged sc-lint boundary rules (R5), hermetic
test/smoke support (R6), and release cadence/compatibility (R7).

No sprints are cut. The plan exists only on branch `plan/phase-ar`
(`docs/plans/phase-AR/`), pending arch-ctm review; it has not merged to
`develop`. Note: the local worktree named `plan/phase-ar-graft-registration`
does NOT contain this plan.

## 51. Phase AS — Shared Publish-Kit Migration [SUPERSEDED — REVERTED, REPLACED BY PHASE AT]

Phase AS attempted to adopt the shared `sc-publish` kit as an upstream-owned
overlay for ATM's release pipeline (crates.io/PyPI/Homebrew/winget). Six
sprints (AS.1–AS.6) were planned and work through AS.4/AS.5 merged into
`integrate/phase-as`, but the entire line was reverted from `develop` via
PR #960 (2026-08-19/20). Phase AT is its successor and explicitly carries
forward no AS work, files, acceptance criteria, or release receipts. The AS
artifacts remain only on `origin/integrate/phase-as` as historical
reference; do not build on them.

## 52. Phase AT — Manifest-Driven Publishing Recovery [COMPLETE — AT.1 MERGED, AT.2 DEFERRALS RECORDED]

Phase AT restarts the publish-kit adoption from the post-AS-revert baseline
(commit `d610b4c07`) under the ADR-050 ownership split: `sc-publish` owns
generic publish mechanics; atm-core owns its own manifest, validation, and
publish-order correctness. Two sprints: `AT.1` install one immutable,
pinned `sc-publish` revision (never patched locally) driven by ATM's
complete consumer JSON input, prove preflight/release parity, and perform
the authorized publish proof; `AT.2` verify coverage, then delete the
legacy publish surface — `release.yml`, `release-preflight.yml`,
`hermes-atm-pypi-publish.yml`, root `scripts/` — that the kit now covers
(must_follow AT.1), retaining rows whose gate receipt does not yet exist as
`deferred-until-<gate>`. Integration branch: `integrate/phase-at`.

**Amendment (2026-08-27, owner decision — forward-only publishing).** The
originally planned TestPyPI→PyPI retry of the pre-kit 1.4.3 release is
withdrawn: v1.4.3 is unpublishable via the kit (kit action absent at the
tag; legacy manifest fails the kit schema), and Rand ruled pre-kit-tag
republishing out of scope. The publish-proof leg is retargeted to the first
kit-era tag (workspace version 1.4.4), cut after phase AT merges to
`develop`; TestPyPI authorization carries over, production remains
pending contemporaneous authorization. See
`docs/plans/phase-at/receipts/AT.1-receipt.md`.

AT.1 merged via PR #1044 (plus receipt amendments #1052/#1062); an AT.1
dry-run rehearsal receipt exists on `smoke/phase-at-at1-rehearsal`. AT.2
completed on `feature/pat-s2-legacy-publish-deletion`: it removed the
unreachable pre-kit installed-doc validator path and recorded every remaining
legacy publish candidate as a gate-bound deferral in its receipt. The first
kit-era release is the follow-up deletion trigger.

## 52a. Phase AS (release validation 1.4.x) — Prerelease Archives [ACTIVE]

This is a distinct, narrowly-scoped release-validation phase, not a revival
of the superseded **Phase AS — Shared Publish-Kit Migration** above. It owns
only the repository-local, tag-triggered prerelease archive workflow and its
operator safeguards. Its current sprint is
[`AS1.1 — Prerelease archive job`](./plans/phase-as/sprint-AS1-1-prerelease-archive.md),
which builds CI-provenanced archives without publishing or modifying the
vendored `sc-publish` kit. It does not inherit the retired phase's files,
acceptance criteria, or release receipts.

## 53. Phase AU — Boundary Debt Retirement ✅ COMPLETE

Phase AU retires the 22 pre-existing sc-boundary findings exposed when
QA-RUSTQA-AO2-001's fix armed the sc-boundary lint in `just validate`
(waived at the AO2 gate as `AO2-SCBOUNDARY-DEBT-001`, tracked in GH #1028) —
the findings currently making CI red on `develop`. Constraint: no boundary
loosening — no rule removed from `just lint all`, no baseline/ignore file;
the only suppressions permitted are the lint's own purpose-built per-type
opt-in markers where they are the designed mechanism. Three sprints mapping
to the analysis waves: `AU.1` mechanical code fixes clearing 9–10 findings
(no lint changes, no design questions); `AU.2` sc-lint-boundary calibration
— NodeId impl-discriminator bug fix, call-callee reference metadata, and
narrowed self-loop/trait-impl classifiers with pinning tests — clearing 11
findings; `AU.3` the one true architectural cycle, `atm_core::ack` ↔
`send`, restructured via a narrow sibling write-contract module (design
review + benchmark-parity gate on m5-atmbench required; mechanical
relocation only on the hot path). Exit: sc-boundary reports 0 findings,
`just validate` fully green, waiver retired, #1028 closed.

The authoritative plan is
[boundary-regression-plan](./plans/boundary-regression-plan.md) (arch-ctm
critical review round 1 folded in); phase-au sprint docs are being cut from
it under `docs/plans/phase-au/` on branch `plan/boundary-regression`.

## 54. Phase AV — Async Mailbox-Read Cutover Completion [COMPLETE — INTEGRATION PR #1120]

Phase AV fixes the mailbox-read serialization regression: every core job —
including all reads — funnels through one single-permit `BlockingCoreBridge`
in `atm-http-runtime`, so an unrelated slow job head-of-line blocks `atm
read` past its client budget. The phase completes the Tokio cutover the AL
phase left unfinished: a bounded read-only WAL reader lane
(`AsyncMailboxReader`), atomic read-handler cutover with the hidden
read-flow mutations split onto the writer lane, normative
requirements/ADR hardening making re-serialization non-compliant,
mechanical hard gates, and massively-parallel read/query benchmark
families with ratcheted floors. Five sprints: `AV.1a` reader-lane
foundation (runtime-inert), `AV.1b` read-handler cutover (the atomic
behavior change), `AV.2` requirements/ADR hardening, `AV.3` mechanical
hard gates, `AV.4` read/query benchmarks. Dependency chain:
AV.1a→AV.1b→{AV.3, AV.4}; AV.2 parallel-safe with all.

The authoritative plan is
[phase-av-plan](./plans/phase-av/phase-av-plan.md) with per-sprint docs
under `docs/plans/phase-av/`, authored on branch `plan/phase-av` (PR #1108).

Phase AV sprint status:

| Sprint | Status | Branch | Artifacts |
| --- | --- | --- | --- |
| `AV.1a` | `merged` (PR #1112) | `fix/mailbox-read-blocking-serialization` | `docs/plans/phase-av/sprint-AV.1a-reader-lane-foundation.md` |
| `AV.1b` | `merged` (PR #1115) | `feature/av1b-read-handler-cutover` | `docs/plans/phase-av/sprint-AV.1b-read-handler-cutover.md` |
| `AV.2` | `complete` | `feature/av2-read-concurrency-requirements` | `docs/requirements.md`, `docs/adr/ADR-059-async-mailbox-read-concurrency.md`, `docs/plans/phase-av/av-closeout-record.md` |
| `AV.3` | `complete` (PR #1113 merged) | `feature/av3-read-concurrency-gates` | `docs/plans/phase-av/sprint-AV.3-mechanical-hard-gates.md` |
| `AV.4` | `complete` (PR #1114 merged) | `feature/av4-read-query-benchmarks` | `docs/plans/phase-av/sprint-AV.4-read-query-benchmarks.md` |

## 55. Phase AX — Nudge Templates On Every Backend And Task-State Tracking [PLANNING — PLAN HARDENING]

Phase AX closes three delivery defects found in the 2026-09-04 Herdr
dogfood run (issue #1173): the Herdr sink bypasses the built-in nudge
templates and injects fixed wake text; `atm queue` has no template class
of its own; and task-tagged mail has no state, so a second task can be
acked while the first is in progress and an idle assignee is never
reminded. Seven sprints in four tracks: A = `AX.1` queue template class
→ `AX.2` Herdr template rendering; B = `AX.3` task state machine and
storage → `AX.4` task CLI and docs, **running in parallel with A**
(`parallel_safe`: AX.1∥AX.3, AX.2∥AX.3); C = `AX.5` reminder cycle →
`AX.6` lead notification and doctor, after A and B merge; D = `AX.7`
live Herdr evidence. `must_follow` edges: AX.1→AX.2, AX.3→AX.4,
AX.2→AX.5, AX.4→AX.5, AX.5→AX.6, AX.6→AX.7. Branches are worktrees via
`sc-git-worktree`; PR bases and merges via `gh stack` (sequence in the
phase plan §6).

The authoritative plan is
[phase-ax-plan](./plans/phase-ax/phase-ax-plan.md) with per-sprint docs
under `docs/plans/phase-ax/`, authored on branch `integrate/phase-ax`.

Phase AX sprint status:

| Sprint | Track | Execute | Status | Branch | Artifacts |
| --- | --- | --- | --- | --- | --- |
| `AX.1` | A | parallel with AX.3/AX.4 | `planned` | `feature/ax1-queue-template-class` | `docs/plans/phase-ax/sprint-AX.1-queue-template-class.md`, ADR-019 amendment |
| `AX.2` | A | after AX.1; parallel with AX.3/AX.4 | `planned` | `feature/ax2-herdr-template-rendering` | `docs/plans/phase-ax/sprint-AX.2-herdr-template-rendering.md`, ADR-058 amendment, `boundaries/atm-herdr/herdr-process-adapter.toml`, `docs/atm-herdr/requirements.md` |
| `AX.3` | B | parallel with AX.1/AX.2 | `complete` | `feature/ax3-task-state-machine` | `docs/plans/phase-ax/sprint-AX.3-task-state-machine.md`, `docs/adr/ADR-061-task-state-machine.md`, ADR-054 amendment, `boundaries/atm-storage/task-store.toml`, `boundaries/atm-storage-rusqlite/task-store-sqlite.toml` |
| `AX.4` | B | after AX.3; parallel with AX.1/AX.2 | `planned` | `feature/ax4-task-cli-and-docs` | `docs/plans/phase-ax/sprint-AX.4-task-cli-and-docs.md`, `docs/user-documents/tasks.md` |
| `AX.5` | C | after A and B merge | `planned` | `feature/ax5-task-reminder-cycle` | `docs/plans/phase-ax/sprint-AX.5-task-reminder-cycle.md`, ADR-061 reminder-cycle section |
| `AX.6` | C | after AX.5 | `planned` | `feature/ax6-lead-notification-doctor` | `docs/plans/phase-ax/sprint-AX.6-lead-notification-doctor.md` |
| `AX.7` | D | after AX.6 merges | `planned` | none (proof on `integrate/phase-ax`) | `docs/plans/phase-ax/ax7-live-proof.md` |

## Publishing Improvements

Implementation Branches:

| Sprint | Status | Branch | Artifacts |
| --- | --- | --- | --- |
| `PI.1` | `complete` | `feature/pPI-s1-validation-infra` | `Justfile`, `.just/print_help.py`, `scripts/validate_release.py`, `scripts/verify_release_archive.py`, `scripts/release_artifacts.py`, `release/publish-artifacts.toml`, `release/RELEASE-NOTES-TEMPLATE.md`, `.github/workflows/release-preflight.yml`, `.github/workflows/release.yml` |
| `PI.2` | `complete` | `integrate/publish-release-readiness` | `.claude/agents/publisher.md`, `docs/release-preflight-checklist.md` |
| `PI.3` | `complete` | `integrate/publish-release-readiness` | `.claude/agents/publisher.md`, `docs/release-preflight-checklist.md`, `.claude/commands/preflight.md` |

Authoritative sprint plan:
- `docs/plans/preflight-documentation/sprint-preflight.md`

## Release Preflight Documentation

Implementation Branches:

| Sprint | Status | Branch | Artifacts |
| --- | --- | --- | --- |
| `PREFLIGHT` | `complete` | `docs/preflight-documentation` | `docs/plans/preflight-documentation/sprint-preflight.md`, `docs/release-preflight-checklist.md`, `.claude/commands/preflight.md`, `.claude/agents/publisher.md` |
