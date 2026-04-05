# ATM CLI Project Plan

## 1. Goal

Implement a daemon-free ATM rewrite in this repo that preserves retained `send`, `read`, `ack`, `clear`, `log`, and `doctor` functionality.

The authoritative migration document is:
- [`docs/archive/file-migration-plan.md`](./archive/file-migration-plan.md)

This plan sequences the work. File-level migration decisions live in
[`docs/archive/file-migration-plan.md`](./archive/file-migration-plan.md).

Documentation organization and cleanup are governed by
[`documentation-guidelines.md`](./documentation-guidelines.md). As the docs are
restructured, product docs remain in `docs/` and crate-local detail moves into
`docs/atm/` and `docs/atm-core/`.

Status:
- Phases 0 through F and J are complete.
- Phase K is now the latest observability-integration phase and the next active
  delivery focus.
- Phases G and H remain retained-command phases, but their implementation work
  is blocked on Phase K completing the concrete `sc-observability` integration.
- Message schema ownership and metadata normalization are now implemented well
  enough for live shared-inbox adoption, while a separate ATM-native inbox
  remains deferred to a later version.

## 2. Deliverables

- Rust workspace with `crates/atm-core` and `crates/atm`
- daemon-free implementation of `send`, `read`, `ack`, `clear`, `log`, and `doctor`
- preserved non-daemon mail functionality from the current codebase
- explicit two-axis workflow model with three display buckets
- task-linked message metadata with mandatory ack behavior
- structured errors with recovery guidance
- structured logs through `sc-observability`
- retained and new integration tests for the retained command surface
- explicit schema ownership docs for Claude Code, legacy ATM compatibility, and
  forward ATM metadata

## 3. Crates

The implementation remains split across:

- `crates/atm-core`
- `crates/atm`

Crate-local scope detail is owned by:

- [`docs/atm-core/requirements.md`](./atm-core/requirements.md)
- [`docs/atm-core/architecture.md`](./atm-core/architecture.md)
- [`docs/atm/requirements.md`](./atm/requirements.md)
- [`docs/atm/architecture.md`](./atm/architecture.md)

## 4. Work Sequence

### Phase 0: Document Lock [COMPLETE]

Status summary:
- Requirements, architecture, and read-behavior documentation are locked, and
  the migration plan now lives in `docs/archive/`.
- This phase completed without a dedicated PR because it was finished before the
  current atm-core PR sequence began.

Finish and freeze:
- `requirements.md`
- `architecture.md`
- `read-behavior.md`
- `docs/archive/file-migration-plan.md`

Acceptance:
- workflow axes, display buckets, retained command surface, and observability boundary are consistent across all docs
- every retained or excluded source file needed for the retained commands is explicitly listed in `docs/archive/file-migration-plan.md`

### Phase A: `OBS-GAP-1` [COMPLETE]

Status summary:
- The `sc-observability` API gap was catalogued and closed before the ATM log
  and doctor work depends on it.
- This phase is historical context only; it is no longer the gating item for
  retained observability delivery.
- Delivered in PR #1.

Goal:
- verify and close the shared `sc-observability` API gap before ATM depends on it for `atm log` and `atm doctor`

Deliverables:
- ATM-side required capability list
- gap list against current `sc-observability`
- concrete API requests for `arch-obs`
- decision on ATM-owned port-boundary responsibilities versus shared observability responsibilities

Acceptance:
- shared plan exists for emit/query/follow/filter/health support
- no ATM-local ad hoc log query engine is needed

### Phase B: Core Skeleton [COMPLETE]

Status summary:
- The workspace, crate scaffolding, CLI command surface, and documentation gap
  closure were completed and merged.
- Delivered in PRs #2 and #3.

| Sprint | Scope | Required outcome |
| --- | --- | --- |
| B.1 | CLI skeleton | `atm` exposes exactly `send`, `read`, `ack`, `clear`, `log`, `doctor` as clap subcommands and all CI gates pass |
| B.2 | Documentation gap closure | lock the remaining send/read/clear requirements and architecture details before Phase C begins |

Create:
- workspace manifests
- `atm-core`
- `atm`
- placeholder module tree matching the architecture

Acceptance:
- workspace builds
- CLI help shows `send`, `read`, `ack`, `clear`, `log`, and `doctor`
- B.1 and B.2 are both complete before Phase C starts
- requirements and architecture lock the message id, read dedupe, and clear
  eligibility semantics needed for implementation

### Phase C: Low-Level Reuse [COMPLETE]

Status summary:
- Foundational reuse landed for mailbox schema alignment, config/path helpers,
  and the shared `AtmError` / `AtmErrorKind` model.
- Delivered in PRs #4 and #5.

Port retained foundational files first:
- home/path helpers
- config and bridge resolution
- address parsing
- text utilities
- schema types
- mailbox primitives
- hook identity

Acceptance:
- foundational unit tests pass
- no daemon references remain in foundational modules

### Phase D: Send Path [COMPLETE]

Status summary:
- The send service, CLI wiring, observability port adapter, and team-config
  validation are all implemented and merged.
- Delivered in PR #6.

Port send command and support files:
- identity resolution
- file policy
- summary generation
- mailbox append
- ack-required and task-linked message creation
- command output
- observability emission

Acceptance:
- `atm send` feature set works without daemon support
- send JSON and human output match the documented contract

### Phase E: Read Path [COMPLETE]

Status summary:
- The read service now includes `IsoTimestamp`, seen-state handling, queue
  bucket filtering, and the required read-path transitions.
- Delivered in PR #7.

Port read command and support files:
- workflow axis classification
- display bucket mapping
- selection modes
- seen-state behavior
- timeout waiting
- legal state transitions
- command output

Acceptance:
- `atm read` feature set works without daemon support
- workflow axes and display buckets match the requirements
- seen-state semantics match the documented contract

### Phase F: Ack And Clear Path [COMPLETE]

Status summary:
- Ack and clear flows are implemented, the remaining 30 RBP findings were
  closed, and CI isolation hardening was completed for the phase.
- Delivered in PRs #8, #9, and #10.

Port ack and clear command support files:
- acknowledgement transition handling
- reply emission
- clear eligibility computation
- clear dry-run reporting
- command output

Acceptance:
- `atm ack` feature set works without daemon support
- `atm clear` removes only clearable messages
- pending-ack messages remain visible until acknowledgement

### Phase G: Log Path [BLOCKED ON PHASE K]

Status summary:
- The retained `log` command remains a command-phase deliverable, but concrete
  implementation is blocked until Phase K lands the real
  `sc-observability` adapter and shared query/follow integration.

Port and redesign the log command:
- injected observability port usage
- log query/filter/tail behavior
- command output
- integration tests

Acceptance:
- `atm log` works through shared `sc-observability` APIs
- level and field filtering work
- tail mode works
- emit failures remain best-effort for mail commands

### Phase H: Doctor Path [BLOCKED ON PHASE K]

Status summary:
- The retained `doctor` command remains a command-phase deliverable, but
  concrete implementation is blocked until Phase K lands the real
  `sc-observability` health/query integration.

Port and redesign the doctor command:
- local config/path checks
- hook identity checks
- mailbox readiness checks
- observability health and query-readiness checks
- command output

Acceptance:
- `atm doctor` works without daemon support

### Phase I: Cleanup And Hardening

Delete:
- daemon-dependent crates and helpers not retained
- leftover imports from daemon-era surfaces

Add:
- integration tests
- snapshot tests
- config/schema hardening for legacy team records with deterministic recovery
  and precise diagnostics
- documentation polish

Acceptance:
- implementation matches `requirements.md`, `architecture.md`,
  `read-behavior.md`, and `docs/archive/file-migration-plan.md`

### Phase J: Message Schema Normalization [COMPLETE]

Status summary:
- schema ownership, compatibility, and forward metadata rules are now
  documented
- the current live design continues to use the shared Claude inbox surface and
  passed J.5 live validation
- a separate ATM-native inbox is explicitly deferred until after the current
  design is live and proven
- no J.5 runtime blocker was found that forces an immediate inbox split

Goal:
- make the shared inbox design safe to run live by clarifying schema ownership,
  deprecating new ATM-only top-level fields, and defining the forward
  metadata-based ATM schema

Execution model:
- this phase is implemented as a coordinated multi-sprint stream owned by
  `team-lead`
- `team-lead` should orchestrate the sprint sequence, worktree assignments, and
  review hand-offs using the `/codex-orchestration` skill
- sprint execution should not assume a separate ATM-native inbox; all work in
  this phase targets the current shared inbox design

Deliverables:
- explicit schema ownership docs:
  - Claude Code-native schema
  - legacy ATM read-compatibility schema
  - forward ATM metadata schema
- enforcement models for locally owned schema docs
- requirements and architecture rules for:
  - legacy read compatibility
  - metadata-only ATM machine fields going forward
  - ULID-based ATM message identifiers
  - timestamp derivation from ULID creation time
  - additive enrichment of Claude-native messages with ATM metadata
- implementation plan for the initial dedup work:
  - PR #18 idle-notification receiver-side dedup using the Claude-native idle
    payload in `text`
  - consolidation of ATM `message_id` surface canonicalization rules across
    read, ack, and clear
  - migration plan for ATM-authored repair/alert dedup toward `metadata.atm`
- next-version deferral note for a separate ATM-native inbox

Completed sprints:

- `J.1` Schema Ownership Lock
  - land the production schema docs and local enforcement models
  - add source-code and unit-test references back to the owning schema docs
  - acceptance: no ambiguity remains about Claude-native vs ATM-owned vs
    legacy ATM read-compat fields

- `J.2` Native Idle Dedup Implementation
  - implement PR #18 receiver-side idle-notification dedup against the
    Claude-native JSON payload stored in `text`
  - remove or reject any implementation that tries to redefine idle notices as
    an ATM-owned native top-level schema
  - acceptance: at most one unread idle notification per sender remains visible
    in an inbox, with fixtures and tests aligned to the Claude-native schema

- `J.3` Surface Canonicalization Consolidation
  - centralize `message_id` dedup logic used by read, ack, and clear
  - keep current legacy top-level `message_id` behavior read-compatible while
    documenting the later move to `metadata.atm.messageId`
  - acceptance: one shared dedup contract is used across operator-facing
    mailbox surfaces

- `J.4` ATM Alert Metadata Migration Plan
  - migrate the design for ATM-authored repair notices from ad hoc top-level
    fields toward `metadata.atm`
  - explicitly preserve legacy top-level `atmAlertKind` and
    `missingConfigPath` as read-compatible until the runtime migration sprint
    lands
  - keep current alert writes/read-compat behavior stable until the migration
    sprint lands
  - acceptance: requirements and architecture specify the forward metadata
    placement for ATM alert/dedup fields without breaking legacy reads

- `J.5` Live Shared-Inbox Validation
  - exercise the documented shared-inbox design in live/manual flows before any
    ATM-native inbox redesign is considered
  - confirm Claude-context projection limitations, enrichment expectations, and
    ack/dedup operator workflows against real inbox files
  - acceptance: the current shared-inbox design is proven usable enough to
    defer ATM-native inbox work to a later version
  - delivered in:
    [`docs/atm-core/design/live-shared-inbox-validation.md`](./atm-core/design/live-shared-inbox-validation.md)

Acceptance:
- schema ownership is explicit in requirements and architecture
- legacy ATM top-level fields are documented as read-compatible but deprecated
  for new writes
- forward ATM metadata schema requires ULID-based ATM message identifiers
- PR #18 idle-notification dedup is explicitly represented in the implementation
  plan as a Claude-native schema-following sprint
- the phase is organized into explicit sprints orchestrated by `team-lead`
  using `/codex-orchestration`
- the current architecture explicitly defers a separate ATM-native inbox until
  a later version

### Phase K: `sc-observability` Integration [NEXT / LATEST]

Status summary:
- the shared `sc-observability` repo now provides the generic query, follow,
  sink, and health surfaces ATM needs
- ATM still uses a local tracing-based emit-only adapter, so retained
  `atm log` and `atm doctor` are not yet delivered on the shared stack
- this phase replaces the old "shared API gap" framing with concrete ATM-side
  integration work

Goal:
- integrate ATM with the current shared `sc-observability` logging/query/health
  surface in a production-ready way before resuming retained `log` and
  `doctor` delivery

Execution model:
- this phase is implemented as a coordinated multi-sprint stream owned by
  `team-lead`
- `team-lead` should orchestrate the sprint sequence, worktree assignments, and
  review hand-offs using the `/codex-orchestration` skill
- the phase uses the ATM-owned adapter/boundary documented in:
  [`docs/atm-core/design/sc-observability-integration.md`](./atm-core/design/sc-observability-integration.md)
- until `sc-observability` is published, local and CI builds may consume the
  shared crates from a sibling checkout using a repo-local Cargo patch/path
  strategy; committed ATM docs and scripts must not require user-specific
  absolute paths

Planned sprints:

- `K.1` Toolchain And Dependency Alignment
  - align ATM to the shared Rust toolchain floor and current stable pin
  - define the pre-publish local dependency strategy used in developer builds
    and CI
  - acceptance: ATM toolchain/docs/CI strategy is explicit and matches the
    shared repo dependency floor

- `K.2` Observability Port Expansion
  - expand the `atm-core` boundary from emit-only to emit/query/follow/health
  - keep `sc-observability` types out of `atm-core` public APIs
  - acceptance: `atm-core` owns the projected ATM request/result types and a
    synchronous tail session boundary

- `K.3` Concrete Adapter Bootstrap
  - replace the local tracing-only `atm` implementation with a real
    `sc-observability` adapter
  - initialize the shared logger once per CLI process and inject it into
    `atm-core`
  - add terminal failure logging for bootstrap, parse, and core-service error
    paths
  - acceptance: retained mail commands emit through the shared logger and
    preserve best-effort behavior, and failure diagnostics carry stable ATM
    error codes

- `K.4` `atm log` Delivery On Shared Query/Follow
  - implement the retained `log` command over `Logger::query(...)` and
    `Logger::follow(...)`
  - acceptance: snapshot/tail/filtering behavior works through the shared log
    store with integration coverage

- `K.5` `atm doctor` Delivery On Shared Health
  - implement the retained `doctor` command over shared logging/query health
  - acceptance: doctor reports active log path, logging health, and query
    readiness with stable findings

- `K.6` Integration And Live Validation
  - close the command-test gap for observability consumer paths and run one
    live/manual validation pass against a real ATM home
  - close the error-logging gap by verifying CLI/bootstrap/service failures and
    degraded recovery warnings all emit stable ATM-owned error codes
  - acceptance: retained observability commands are proven on the shared stack
    before phase close

Acceptance:
- ATM no longer depends on a local tracing-only observability adapter
- `atm-core` owns an explicit emit/query/follow/health boundary over shared
  observability crates
- local and CI builds use the same documented pre-publish shared-crate
  dependency strategy
- `atm log` and `atm doctor` are implemented on the shared logging/query/health
  stack
- observability command integration coverage exists for snapshot, tail, filter,
  and doctor readiness flows
- any generic shared-crate usability gaps discovered during implementation are
  filed upstream in `sc-observability`

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
- `atm send` works without daemon support
- `atm read` works without daemon support
- `atm ack` works without daemon support
- `atm clear` works without daemon support
- `atm log` works through shared observability APIs
- `atm doctor` works as a local diagnostics command
- retained non-daemon functionality is preserved or intentionally documented as changed
- task-linked mail remains pending until acknowledged
- the file-by-file migration plan is complete enough to implement directly
- the retained command tests pass against the new crate layout

## 7. Documentation Review Checks

Before implementation starts, the docs should be reviewed with these checks:
- every retained or rejected source file referenced by the retained command
  surface appears in `docs/archive/file-migration-plan.md`
- `requirements.md`, `architecture.md`, and `read-behavior.md` agree on the two-axis model, three display buckets, and legal transitions
- `requirements.md`, `architecture.md`, and `read-behavior.md` agree on `--since`, `--since-last-seen`, `--no-since-last-seen`, `--no-update-seen`, and `--timeout`
- `requirements.md`, `architecture.md`, and
  `docs/archive/file-migration-plan.md` agree on the retained command set:
  `send`, `read`, `ack`, `clear`, `log`, `doctor`
