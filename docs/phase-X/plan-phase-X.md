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
- `integrate/phase-W` at `96f197e`
- authoritative inputs:
  - `docs/phase-W/post-mortem.md`
  - `crates/atm-core/src/service_runtime_store.rs`
  - `crates/atm-core/src/ack/mod.rs`
  - `crates/atm-daemon/src/runtime_status_cache.rs`
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm-daemon/src/peer_transport.rs`

Target integration branch:
- `integrate/phase-X`

Predecessor gate:
- Phase `W` must remain merged and validated on `integrate/phase-W`
- no Phase `X` sprint may preserve or add a mailbox durability fallback path

Boundary rules for Phase `X`:
- SQLite/store is the only durable ATM mailbox implementation
- daemon/store unavailability must return shared ATM errors; no runtime path may
  downgrade to file-backed mailbox reads or writes
- file watchers may remain only as ingress/reconcile edges; they must not remain
  a parallel mailbox implementation behind command/runtime traits
- daemon runtime health must not assemble team truth from filesystem discovery
  plus SQLite membership; ownership must be explicit and singular

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
- silent-emit-discard CI lint
- typed observability migration requirements/architecture updates
- RULE-002 function-length CI lint
- infallible-result rust-qa-agent checklist update
- structured-logging guidelines advisory

These are included in `X.5` below.

### Parallel Process Items Owned By `team-lead`

The following post-mortem actions remain outside the engineering sprint line:
- dev-template sprint-doc/project-plan gate updates
- qa-template sprint-doc verification updates
- any team-lead-owned planning-process changes not implemented in repo code

Phase `X` does not absorb those into `arch-ctm` sprint scope.

## Execution Shape

- `X.1` mailbox runtime cutover and dual-mode surface deletion
- `X.2` command-path simplification and legacy mailbox path deletion
- `X.3` daemon runtime truth unification and runtime-status-cache refactor
- `X.4` replay-persistence startup contract and peer-transport decomposition
- `X.5` systemic guardrails, typed-observability docs, and CI/lint follow-up

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

Required deliverables:
- delete `LegacyMailboxRuntime`
- delete `DefaultMailboxRuntime::Legacy`
- delete `legacy_runtime()`
- change `default_runtime()` so it never selects a legacy mailbox runtime
- remove `allows_legacy_mailbox_files()` from the runtime-facing mailbox
  contract
- replace file-backed mailbox trait methods on `RetainedMailboxRuntime` with
  store-shaped operations needed by command logic

Acceptance criteria:
- `rg -n "LegacyMailboxRuntime|DefaultMailboxRuntime::Legacy|legacy_runtime\\(|allows_legacy_mailbox_files"` returns no production-code matches in
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

Required deliverables:
- delete `crates/atm-core/src/read/legacy_path.rs`
- remove production runtime handling of `legacy:` mailbox keys as a normal
  control-flow path
- remove direct source-file lock/read/write branches from `ack`, `read`,
  `clear`, and any shared mailbox append helpers
- keep file-watcher or ingress helpers, if still needed, behind a dedicated
  daemon/private ingress boundary rather than the general command runtime trait
- delete or narrow `boundary_support.rs` file-backed import/export helpers if
  they remain on the production mailbox path

Acceptance criteria:
- `crates/atm-core/src/read/legacy_path.rs` is removed
- `rg -n "observe_source_files|commit_source_files|with_locked_source_files|commit_mailbox_state|read_messages"` finds no command-path use through the
  retained runtime surface in:
  - `crates/atm-core/src/ack/mod.rs`
  - `crates/atm-core/src/read/mod.rs`
  - `crates/atm-core/src/clear/mod.rs`
  - `crates/atm-core/src/send/mod.rs`
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
- `crates/atm-core/src/boundary.rs` or the owning roster boundary module
- `crates/atm-rusqlite/src/lib.rs` and any roster-store implementation files
- `crates/atm-daemon/src/composition.rs` if runtime-state wiring changes

Required deliverables:
- add the boundary operation needed to enumerate daemon teams from the
  authoritative store path
- remove filesystem enumeration of `ATM_HOME/.claude/teams` from
  `build_runtime_status_cache_state(...)`
- make `build_runtime_status_cache_state(...)` explicitly SQLite-owned for both
  team discovery and member discovery
- refactor `build_runtime_status_cache_state(...)` below the RULE-002 `80`-line
  limit

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
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`

Required deliverables:
- document the replay-store startup contract:
  - fail-closed, or
  - explicitly allowed reduced-capability startup
- make `composition.rs` enforce that documented contract instead of leaving
  `replay_store = None` as implicit behavior
- refactor `send_to_endpoint(...)` below `80` lines
- refactor `send_once(...)` below `80` lines

Acceptance criteria:
- one replay-persistence startup contract is documented in product and
  daemon-local docs
- daemon startup behavior in `composition.rs` matches the documented contract
- `send_to_endpoint(...)` is under `80` lines
- `send_once(...)` is under `80` lines
- peer transport preserves the shared ATM error / recovery contract after the
  refactor

Required validation:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`

### `X.5` — Guardrails And Typed-Observability Follow-Through

Goal:
- close the `arch-ctm` systemic follow-ups from the Phase `W` post-mortem
- add mechanical guardrails so deleted fallback or silent-discard patterns do
  not re-enter the codebase

Primary file scope:
- `scripts/check-silent-emit.sh`
- `scripts/check-function-length.py`
- CI workflow files that own repository gate execution
- `docs/requirements.md`
- `docs/architecture.md`
- `.claude/assets/sc-rust/quality-mgr/templates/`
- `.claude/skills/rust-development/guidelines.txt`

Required deliverables:
- add a CI gate for silent `emit()` discard patterns
- add a CI gate for RULE-002 function length with:
  - warning posture for grandfathered existing violations
  - hard fail for new violations introduced by a PR diff
- update `docs/requirements.md` with the remaining typed observability migration
  requirement
- update `docs/architecture.md` with the phased typed observability migration
  strategy and upstream dependency note
- update the rust QA checklist to scan for infallible `Result<T, E>` shapes
- add the structured-logging advisory for daemon `warn!` / `error!` fields

Acceptance criteria:
- the silent-emit-discard gate is runnable in CI
- the RULE-002 gate is runnable in CI
- typed observability completion is explicitly captured in requirements and
  architecture docs
- the rust QA checklist includes the infallible-result review step
- daemon structured-logging guidance is documented in the Rust development
  guidelines

Required validation:
- execute each new script locally in its intended mode
- run the affected CI/lint entrypoints locally if available
- `git diff --check`

## Phase Acceptance

Phase `X` planning is complete when:
- PX-001, PX-002, and PX-003 each map to one explicit implementation sprint
- `ARCH-W-001`, `ARCH-W-002`, and `ARCH-W-003` each have sprint ownership
- every `arch-ctm` systemic follow-up item from the Phase `W` post-mortem has
  sprint ownership
- sprint scopes are deletion-oriented and file-specific rather than generic
  cleanup language
- acceptance criteria are concrete enough for QA to verify without reopening
  planning
