# Phase X — SQLite SSOT And Daemon Boundary Simplification

Goal:
- close the pre-existing SSOT gaps that remained after Phase `W`
- remove the dual mailbox implementation so ATM runtime logic has one durable
  mailbox path and minimal branching
- make daemon runtime truth SQLite-owned where the product claims SQLite SSOT
- decide and enforce the replay-persistence startup contract instead of leaving
  reduced-capability behavior implicit
- land the deferred structural and lint follow-up work needed to keep the same
  regressions from reappearing

Phase scope note:
- Phase `X` is implementation planning only.
- It is not a discovery line.
- Every sprint below is written to be directly executable without a separate
  planning sprint.

Planning branch:
- `feature/pX-s0-planning`

Baseline:
- `integrate/phase-W` at `9016eed`
- authoritative inputs:
  - `docs/phase-W/post-mortem.md`
  - `crates/atm-core/src/service_runtime_store.rs`
  - `crates/atm-core/src/ack/mod.rs`
  - `crates/atm-core/src/read/mod.rs`
  - `crates/atm-core/src/read/legacy_path.rs`
  - `crates/atm-core/src/clear/mod.rs`
  - `crates/atm-core/src/send/mod.rs`
  - `crates/atm-core/src/boundary_support.rs`
  - `crates/atm-core/src/mailbox/store.rs`
  - `crates/atm-daemon/src/runtime_status_cache.rs`
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm-daemon/src/peer_transport.rs`
  - `crates/atm-daemon-client/src/lib.rs`
  - `crates/atm/src/composition.rs`
  - `crates/atm-graft/src/lib.rs`
  - `crates/atm-graft/src/runtime.rs`
  - `crates/atm-graft/src/transport.rs`
  - `docs/requirements.md`
  - `docs/architecture.md`
  - `.claude/assets/sc-rust/quality-mgr/templates/rust-qa-assignment.json.j2`
  - `.claude/skills/rust-development/guidelines.txt`
  - `.claude/skills/codex-orchestration/dev-template.xml.j2`
  - `.claude/skills/codex-orchestration/qa-template.xml.j2`

Target integration branch:
- `integrate/phase-X`

Predecessor gate:
- Phase `W` must remain merged and validated on `integrate/phase-W`
- no Phase `X` sprint may preserve or add a mailbox durability fallback path

Pre-phase prerequisite:
- before `integrate/phase-X` is created, the following guardrails must land on
  `develop` through a standalone branch such as `feature/pX-lint-gates`:
  - `scripts/check-silent-emit.py`
  - `scripts/check-function-length.py`
- reason:
  - these gates must already be live on every Phase `X` sprint branch from its
    first push
  - they are not acceptable as a late sprint inside `integrate/phase-X`

Boundary rules for Phase `X`:
- SQLite/store is the only durable ATM mailbox implementation
- daemon/store unavailability must return shared ATM errors; no runtime path may
  downgrade to file-backed mailbox reads or writes
- no retained command/runtime path may call source-file mailbox helpers directly
  once the SQLite cutover sprint line begins
- `legacy:` mailbox-key handling is not an allowed production compatibility
  branch after the Phase `X` deletion line lands
- file watchers may remain only as ingress/reconcile edges; they must not remain
  a parallel mailbox implementation behind command/runtime traits
- daemon runtime health must not assemble team truth from filesystem discovery
  plus SQLite membership; ownership must be explicit and singular
- same-host daemon client connection/exchange/error helpers must have one shared
  implementation line; `atm` and `atm-graft` may not retain duplicate helper
  stacks after the Phase `X` closeout
- any sprint that touches shared CLI / graft / peer failure paths must name the
  shared ATM error / protocol / doctor surface it is preserving and the current
  baseline behavior it must not regress
- dependency manifests must reflect real target ownership; helper relocation or
  `#[path = ...]` indirection may not leave stale dependencies hidden until
  `cargo-shear` catches them at the end of a sprint

## Current-State Analysis

### PX-001 [BLOCKING for SSOT] — Mailbox durability still has two implementations

Current code still exposes both SQLite and legacy file-backed mailbox behavior:
- `crates/atm-core/src/service_runtime_store.rs`
  - `DefaultMailboxRuntime::{Sqlite, Legacy}` at lines `19-22`
  - `default_runtime()` fallback to `Legacy(...)` at lines `51-55`
  - retained runtime still fronts file-backed mailbox operations at lines
    `299-345`
  - `LegacyMailboxRuntime` remains live at lines `606-689`
- `crates/atm-core/src/ack/mod.rs`
  - command logic still branches into the legacy file path at lines `154-328`

Required outcome:
- delete the second mailbox implementation from the runtime surface
- delete runtime branching on mailbox backend choice
- make command/runtime mailbox operations store-shaped only

### PX-002 [HIGH] — Daemon runtime truth is still hybrid filesystem plus SQLite

Current code still assembles daemon runtime state from two ownership domains:
- `crates/atm-daemon/src/runtime_status_cache.rs`
  - `build_runtime_status_cache_state(...)` at lines `393-494`
  - team discovery starts from `ATM_HOME/.claude/teams`
  - member truth then comes from the SQLite-backed roster store

Required outcome:
- one owner for daemon team/runtime discovery
- `build_runtime_status_cache_state(...)` reduced below the RULE-002 threshold

### PX-003 [MEDIUM] — Replay persistence startup contract is implicit

Current code allows reduced-capability startup without an explicit product
decision:
- `crates/atm-daemon/src/composition.rs`
  - replay-store assembly failure at lines `186-202`
  - daemon logs degradation and continues with `replay_store = None`

Required outcome:
- decide whether replay persistence is fail-closed or allowed reduced-capability
- encode that decision in requirements/architecture and startup behavior

### PX-004 [HIGH] — Same-host client helper duplication still exists

Current code still keeps duplicated same-host daemon helper logic across the
CLI and graft surfaces:
- `crates/atm/src/composition.rs`
  - local `try_connect(...)`
  - local `exchange(...)`
  - local `unexpected_response(...)`
- `crates/atm-graft/src/transport.rs`
  - duplicated `try_connect(...)`
  - duplicated `exchange(...)`
  - duplicated `unexpected_response(...)`

Required outcome:
- one shared same-host client helper line
- one shared error/envelope mapping line for these interfaces
- no drift between CLI and graft behavior for daemon unavailable or unexpected
  response handling

### Deferred Phase W Structural Findings

The following deferred findings are mandatory Phase `X` obligations:
- `ARCH-W-001`
  - `crates/atm-daemon/src/peer_transport.rs`
  - `send_to_endpoint(...)` at lines `296-403`
  - `108` lines; exceeds RULE-002 `80`-line limit
- `ARCH-W-002`
  - `crates/atm-daemon/src/peer_transport.rs`
  - `send_once(...)` at lines `405-527`
  - `123` lines; exceeds RULE-002 `80`-line limit
- `ARCH-W-003`
  - `crates/atm-daemon/src/runtime_status_cache.rs`
  - `build_runtime_status_cache_state(...)` at lines `393-494`
  - `102` lines; exceeds RULE-002 `80`-line limit

### Systemic Follow-Up Items Owned By `arch-ctm`

Phase `W` post-mortem assigned these follow-ups to `arch-ctm`:
- typed observability migration requirements/architecture updates
- infallible-result rust-qa-agent checklist update
- structured-logging guidelines advisory
- dependency-ownership validation so helper relocation does not leave stale
  manifest entries behind

The silent-emit and RULE-002 lint gates are pre-phase develop-targeting
prerequisites. The remaining items are already-landed baseline verification
inputs or active `X.5` closeout work; they are not separate new typed-doc
deliverables on `integrate/phase-X`.

## Execution Shape

- pre-phase prerequisite:
  - standalone develop-targeting lint-gate PR before `integrate/phase-X`
- `X.1` mailbox runtime cutover and dual-mode surface deletion
- `X.2` command-path simplification and legacy mailbox path deletion
- `X.3` daemon runtime truth unification and runtime-status-cache refactor
- `X.4` replay-persistence startup contract, peer-transport decomposition, and
  same-host IPC helper consolidation
- `X.5` systemic guardrails, dependency-ownership validation, and closeout
  verification

## Sprint Ownership

### `X.1` — Mailbox Runtime Cutover

Goal:
- remove dual mailbox runtime selection from the ATM core runtime surface
- replace mailbox backend choice with one SQLite/store-backed runtime path

Primary file scope:
- `crates/atm-core/src/service_runtime_store.rs`
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/send/mod.rs`
- `docs/atm-core/boundaries.md`
- `docs/atm-core/architecture.md`

Required deliverables:
- delete `LegacyMailboxRuntime`
- delete `DefaultMailboxRuntime::Legacy`
- delete `legacy_runtime()`
- delete the internal legacy-only helpers in
  `service_runtime_store.rs:112-185`:
  - `encode_legacy_message_key()`
  - `decode_legacy_message_key()`
  - `legacy_query_mailbox_metadata_rows()`
  - `legacy_load_message_record()`
- change `default_runtime()` so it never selects a legacy mailbox runtime
- remove `allows_legacy_mailbox_files()` from the runtime-facing mailbox
  contract
- replace file-backed mailbox trait methods on `RetainedMailboxRuntime` with
  store-shaped operations needed by command logic
- remove `LEGACY_MESSAGE_KEY_PREFIX` and any production runtime dependency on a
  dual mailbox-runtime discriminant
- update `docs/atm-core/boundaries.md` and `docs/atm-core/architecture.md` so
  the retained runtime boundary documents one durable mailbox backend only

Acceptance criteria:
- `rg -n "LegacyMailboxRuntime|DefaultMailboxRuntime::Legacy|legacy_runtime\\(|allows_legacy_mailbox_files"` returns no production-code matches in
  `crates/atm-core/src`
- `rg -n "LEGACY_MESSAGE_KEY_PREFIX"` returns no production-code matches in
  `crates/atm-core/src`
- `default_runtime()` no longer returns a legacy mailbox implementation
- command/runtime mailbox interfaces no longer expose backend-choice branching
- any daemon/store unavailability on the mailbox path returns shared ATM errors
  instead of selecting a second implementation

Required validation:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`

### `X.2` — Command Path Simplification And Legacy Path Deletion

Goal:
- remove the remaining file-backed mailbox command branches so runtime logic
  becomes minimal and store-only

Primary file scope:
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/read/legacy_path.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `docs/atm-core/boundaries.md`

Required deliverables:
- delete `crates/atm-core/src/read/legacy_path.rs`
- remove production runtime handling of `legacy:` mailbox keys as a normal
  control-flow path
- remove direct source-file lock/read/write branches from `ack`, `read`,
  `clear`, and any shared mailbox append helpers
- if any file-watcher or ingress helpers survive the sprint, move them behind a
  dedicated daemon-private ingress boundary rather than the general command
  runtime trait
- delete or narrow `boundary_support.rs` file-backed import/export helpers if
  they remain on the production mailbox path
- delete store-facing helpers from the retained runtime surface when they only
  exist to support legacy file-backed mailbox mutation
- remove the retained boundary-adapter stubs in
  `service_runtime_store.rs:305-315`; they are deletion targets, not
  documentation anchors to preserve after the store-only cutover
- document any remaining source-file helper ownership as daemon-private
  ingress/migration-only scope rather than retained runtime scope

Acceptance criteria:
- `crates/atm-core/src/read/legacy_path.rs` is removed
- `rg -n "observe_source_files|commit_source_files|with_locked_source_files|commit_mailbox_state|read_messages"` finds no command-path use through the
  retained runtime surface in:
  - `crates/atm-core/src/ack/mod.rs`
  - `crates/atm-core/src/read/mod.rs`
  - `crates/atm-core/src/clear/mod.rs`
  - `crates/atm-core/src/send/mod.rs`
- `rg -n "legacy:" crates/atm-core/src` returns no production compatibility
  branch matches outside explicitly retained test fixtures
- `rg -n "observe_source_files|commit_source_files|with_locked_source_files|commit_mailbox_state|read_messages" crates/atm-core/src` finds no
  production use outside:
  - explicitly retained daemon-private ingress or migration modules, or
  - tests
- command logic no longer branches on mailbox backend selection
- mailbox command behavior remains routed through one store-backed path

Required validation:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`

### `X.3` — Daemon Runtime Truth Unification

Goal:
- make daemon runtime team/member truth come from one ownership model
- close PX-002 and the deferred `ARCH-W-003` function-length finding together

Primary file scope:
- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm-rusqlite/src/lib.rs` and any roster-store implementation files
- `crates/atm-daemon/src/composition.rs`
- `docs/atm-core/boundaries.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-daemon/architecture.md`

Required deliverables:
- add the boundary operation needed to enumerate daemon teams from the
  authoritative store path
- remove filesystem enumeration of `ATM_HOME/.claude/teams` from
  `build_runtime_status_cache_state(...)`
- make `build_runtime_status_cache_state(...)` explicitly SQLite-owned for both
  team discovery and member discovery
- keep `evict_status_cache_entry_if_needed()` in the named refactor surface so
  helper extraction does not silently change the existing bounded-cap eviction
  and conflict-preservation behavior
- refactor `build_runtime_status_cache_state(...)` below the RULE-002 `80`-line
  limit
- explicitly treat the `runtime_health.rs:47-110` shutdown-finalizer thread
  registry as out of scope for `X.3` unless the runtime-truth rewiring forces a
  lifecycle integration change; QA should not flag that registry as missing
  `X.3` work by default
- update daemon boundary docs so runtime health/status ownership no longer
  implies direct filesystem discovery

Acceptance criteria:
- no `read_dir(.../.claude/teams...)` based team discovery remains in
  `build_runtime_status_cache_state(...)`
- daemon runtime status assembly uses one explicit roster/store source for team
  and member truth
- `build_runtime_status_cache_state(...)` is under `80` lines
- doctor/runtime-health still surface SQLite unavailability and degraded state
  with the existing shared ATM error contract

Required validation:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`

### `X.4` — Replay Persistence Contract And Peer Transport Refactor

Goal:
- make replay persistence startup behavior explicit and enforceable
- close PX-003 together with deferred peer-transport RULE-002 violations

Primary file scope:
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/peer_transport.rs`
- `crates/atm-daemon-client/src/lib.rs`
- `crates/atm/src/composition.rs`
- `crates/atm-graft/src/lib.rs`
- `crates/atm-graft/src/runtime.rs`
- `crates/atm-graft/src/transport.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon-client/boundaries.md`
- `boundaries/atm-daemon-client/daemon-bootstrap.toml`

Required deliverables:
- document the replay-store startup contract:
  - fail-closed, or
  - explicitly allowed reduced-capability startup
- make `composition.rs` enforce that documented contract instead of leaving
  `replay_store = None` as implicit behavior
- refactor `send_to_endpoint(...)` below `80` lines
- refactor `send_once(...)` below `80` lines
- consolidate same-host `try_connect(...)`, `exchange(...)`, and
  `unexpected_response(...)` ownership onto the shared daemon-client line
- consolidate duplicate daemon-unavailable / unexpected-response behavior across
  `atm` and `atm-graft`
- update `docs/atm-daemon-client/boundaries.md` so the boundary contract
  explicitly allows daemon-client to own the shared same-host transport helpers
  (`try_connect`, `exchange`, and `unexpected_response`) after the
  consolidation
- update `boundaries/atm-daemon-client/daemon-bootstrap.toml` so the
  machine-readable boundary contract matches that helper ownership change

Acceptance criteria:
- one replay-persistence startup contract is documented in product and
  daemon-local docs
- daemon startup behavior in `composition.rs` matches the documented contract
- `send_to_endpoint(...)` is under `80` lines
- `send_once(...)` is under `80` lines
- peer transport preserves the shared ATM error / recovery contract after the
  refactor
- `rg -n "fn try_connect\\(|fn exchange\\(|fn unexpected_response\\(" crates/atm crates/atm-graft crates/atm-daemon-client`
  finds one shared helper definition per helper name after the refactor
- CLI and graft same-host paths share the same daemon-unavailable and
  unexpected-response behavior

Required validation:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`

### `X.5` — Guardrails, Dependency Ownership, And Closeout Verification

Goal:
- close the `arch-ctm` systemic follow-ups from the Phase `W` post-mortem
- add mechanical guardrails so deleted fallback or silent-discard patterns do
  not re-enter the codebase

Primary file scope:
- `scripts/check-legacy-mailbox-paths.sh`
- `scripts/check-capability-degradation.sh`
- CI workflow files that own repository gate execution
- `.claude/assets/sc-rust/quality-mgr/templates/`
- `.claude/skills/rust-development/guidelines.txt`

Required deliverables:
- add a CI gate for mailbox-legacy deletion regressions covering:
  - `LegacyMailboxRuntime`
  - `DefaultMailboxRuntime::Legacy`
  - `legacy_runtime()`
  - `allows_legacy_mailbox_files()`
  - `legacy:` production mailbox-key branches
  - command/runtime use of source-file mailbox helper APIs
- add a CI gate preventing replay-capability degradation regressions after
  `X.4`, with a no-production-match search for:
  - `replay_store = None`
  - `replay_store: None`
- wire the pre-phase silent-emit and RULE-002 guards into the documented local
  lint/CI entrypoints used by Phase `X` branches, rather than re-implementing
  them on the integration line
- add dependency-ownership validation to the local lint/CI path, including
  `cargo-shear`, so helper relocation or `#[path = ...]` indirection cannot
  leave stale dependency declarations until end-of-phase review
- verify the following already-landed baseline artifacts from `TASK-1515`
  remain present and aligned with the final Phase `X` closeout:
  - `docs/requirements.md` typed observability migration requirement
  - `docs/architecture.md` phased typed observability migration note
  - rust QA checklist coverage for infallible `Result<T, E>` review
  - structured-logging advisory for daemon `warn!` / `error!` fields
- update QA/checklist language so deletion sprints must search the entire
  workspace for the removed legacy pattern family, not only the touched files

Acceptance criteria:
- the legacy-mailbox-regression gate is runnable in CI
- the replay-capability-degradation regression gate is runnable in CI
- the pre-phase silent-emit and RULE-002 gates are referenced as already-live
  branch prerequisites for all Phase `X` sprint branches
- the local lint entrypoints include dependency-ownership validation
- the already-landed `TASK-1515` baseline artifacts remain present and
  consistent with the final Phase `X` closeout:
  - typed observability requirement in `docs/requirements.md`
  - phased typed observability note in `docs/architecture.md`
  - infallible-result review step in the rust QA checklist
  - daemon structured-logging guidance in the Rust development guidelines
- deletion-sprint QA instructions explicitly require whole-workspace pattern
  searches for removed legacy constructs

Required validation:
- execute each new script locally in its intended mode
- run the affected CI/lint entrypoints locally, or record the exact entrypoint
  that is unavailable in the sprint validation report
- run `cargo-shear`
- `git diff --check`

## Phase Acceptance

Phase `X` planning is complete when:
- PX-001 maps to `X.1`
- PX-002 maps to `X.3`
- PX-003 maps to `X.4`
- PX-004 maps to `X.4`
- `ARCH-W-001` maps to `X.4`
- `ARCH-W-002` maps to `X.4`
- `ARCH-W-003` maps to `X.3`
- every `arch-ctm` systemic follow-up item from the Phase `W` post-mortem has
  sprint ownership
- same-host CLI/graft helper deduplication has explicit sprint ownership
- shared ATM error / protocol / doctor reuse points are named where a sprint
  changes multi-interface behavior
- sprint scopes are deletion-oriented and file-specific rather than generic
  cleanup language
- acceptance criteria are concrete enough for QA to verify without reopening
  planning
