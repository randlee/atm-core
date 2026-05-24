# Phase Z Readiness

## Purpose

Final release-signoff record for `Phase Z`.

## Record Schema

Each sprint row must record:

- `sprint`
- `accepted_commit`
- `verdict`
- `current_status`
- `notes`

Sprint planning status convention:

- sprint docs remain `status: planned` until execution closes the sprint on the
  implementation line

## Final Verdict Record

The final section of this document must record:

- `integrate_phase_z_candidate`
- `release_checklist_result`
- `release_verdict`
- `authorized_by` (explicit named authority limited to `team-lead` by ATM
  identity or explicit user approval recorded by name, plus the approving ATM
  message id or equivalent named artifact)
- `notes`

The final release verdict must remain `PENDING` until:

- `docs/phase-Z/release-checklist.md` records a final checklist result for the
  closeout candidate
- every row in `docs/phase-Z/canary-findings-ledger.md` records a final
  `z4_disposition`
- every deferred `Z.3` finding records explicit `team-lead` approval

## Initial State

| Sprint | Accepted Commit | Verdict | Current Status | Notes |
| --- | --- | --- | --- | --- |
| Z.1 | `70f4fa7f` | `FAIL` | `complete` | smoke checklist frozen; two blocking findings promoted to `Z.2`; cargo test --workspace PASS (CI: macOS/Ubuntu/Windows); git diff --check PASS (CI: Format check) |
| Z.2 | `f412959d` | `PASS` | `complete` | `Z.16` reran the frozen `Z.1` matrix after the `Z.11` through `Z.15` fix line. `Z1-F002` is closed, the copied-state lane no longer fails in SQLite schema init, clean-room retained command/send flows pass, and retry-visible daemon/runtime behavior is observable in `log snapshot`. |
| Z.5 | `4531eaea` | `PASS` | `complete` | retained command membership checks now use ATM roster truth only; doctor drift warnings compare Claude file members against ATM roster truth; `cargo test --workspace` PASS; `cargo fmt --all --check` PASS; production grep gate leaves only doctor compare-only and test-only `load_team_config(...)` matches; `ack_mail_uses_atm_roster_truth_for_valid_actor` PASS |
| Z.6 | `3cd6fb74` | `PASS` | `complete` | Claude send no longer uses pre-write `config.json` membership gating; immutable `ClaudeCodeTeamRoster` now drives the post-write warning path from store-backed ATM roster rows; `cargo test --workspace` PASS; targeted `z6_post_write_warning_uses_store_backed_claude_roster` PASS; production grep gate leaves no `load_team_config(...)` matches in `send` or `service_runtime`; `cargo fmt --all --check` PASS; `git diff --check` PASS |
| Z.7 | `1fd6ee1e` | `PASS` | `complete` | `ConfigIngress` no longer exposes generic team-config lookup; daemon/core forwarding chain removed; startup-only `hydrate_roster_from_team_config_once_at_startup_if_empty(...)` allowlisted through `Z.8` and proven to no-op when roster state is non-empty; `just lint boundaries` PASS with fixture self-test; `cargo test --workspace` PASS; `cargo fmt --all --check` PASS; `git diff --check` PASS |
| Z.8 | `84229892` | `PASS` | `complete` | watcher / reconcile now owns external Claude `config.json` ingest; startup-only bootstrap helper deleted; process-local projection-write suppression skips one matching daemon-authored event and clears on restart; `cargo test --workspace` PASS; targeted `z8_watcher_ingest_hydrates_atm_roster_truth_for_new_team` PASS; targeted `z8_projection_write_suppression_is_process_local` PASS; targeted `z8_deletes_startup_only_config_bootstrap_helper` PASS; `just lint boundaries` PASS; `cargo fmt --all --check` PASS; `git diff --check` PASS |
| Z.9 | `903cbfe1` | `PASS` | `complete` | team-admin commands now use ATM roster truth; `add-member` mutates canonical ATM roster first and projects `config.json`; canonical pane metadata maps ATM `recipient_pane_id` to Claude `tmux_pane_id`; restore preserves canonical `tmux_pane_id`; `cargo test --workspace` PASS; `cargo fmt --all` PASS; `just lint boundaries` PASS; `git diff --check` PASS |
| Z.10 | `c32a0277` | `PASS` | `complete` | backup now writes `atm-roster.json` as an audit-only canonical ATM roster snapshot; restore rebuilds recreated `config.json` from ATM roster truth instead of replaying backup `config.json`; reconcile runtime helpers split into submodules; `cargo test --workspace` PASS; `cargo fmt --all --check` PASS; `just lint boundaries` PASS; `git diff --check` PASS |
| Z.11 | `1bcf916e` | `PASS` | `complete` | clean-start first-send failure now returns the exact actionable recovery contract with no hidden fallback; local non-Claude outbound fallback now enforces the documented payload-size gate; `RSH-008` and `RSH-010` closed; targeted `missing_roster_member_returns_actionable_recovery_contract`, `z11_empty_atm_roster_failure_is_actionable_without_fallback`, and `local_non_claude_outbound_rejects_oversized_payloads` PASS |
| Z.12 | `602292e3` | `PASS` | `complete` | retained roster inspection and mutation commands now route only through the approved `RosterStore` seam; `SCB-RETAINED-001` rejects direct command-entry / team-admin `service_runtime_store::default_runtime()` misuse; clean-room proof shows `atm teams --json`, `atm teams add-member z12-team z12-operator --json`, and `atm members --team z12-team --json` succeed without an installed default runtime factory; `cargo test --workspace` PASS; `cargo clippy --workspace --all-targets -- -D warnings` PASS; `cargo fmt --all --check` PASS; `just lint boundaries` PASS; `git diff --check` PASS |
| Z.13 | `4b3c7506` | `PASS` | `complete` | team-admin current-team resolution now reaches workspace config only through the approved `ConfigIngress` request/response seam; `SCB-WORKSPACE-001` rejects direct command/team-admin `load_config(...)` regressions; production grep leaves zero `load_config(...)` matches in `team_admin.rs`, `teams.rs`, and `members.rs`; `cargo test --workspace` PASS; `cargo clippy --workspace --all-targets -- -D warnings` PASS; `cargo fmt --all --check` PASS; `just lint boundaries` PASS; `git diff --check` PASS |
| Z.14 | `ae13055b` | `PASS` | `complete` | public crate-root runtime-factory installation re-exports are gone; surviving installs are routed only through bounded hidden hooks and approved wrappers; `SCB-SINGLETON-001` now rejects new ambient singleton exposure with fixture self-test and `just lint boundaries` PASS; `cargo test --workspace` PASS; `cargo clippy --workspace --all-targets -- -D warnings` PASS; `cargo fmt --all --check` PASS; `git diff --check` PASS |
| Z.15 | `7b6f9491` | `PASS` | `complete` | typed `AgentType` / `ModelName` / `PaneId` surfaces now replace the remaining raw roster/config strings; `AddMemberRequest` metadata is typed and normalized; replayed local IPC and projection-write follow-up remains explicit and covered by the accepted runtime/daemon test line; `cargo test --workspace` PASS; `cargo clippy --workspace --all-targets -- -D warnings` PASS; `cargo fmt --all --check` PASS; `just lint boundaries` PASS; `git diff --check` PASS |
| Z.18 | `ae1b753c` | `PASS` | `complete` | smoke skill scaffold, canonical JSON contract, shared fixtures/helper layer, J2 report templates, tracked-latest `reports/smoke/` snapshots, and gitignored timestamped smoke artifacts now land without claiming fast/normal/thorough execution closure yet |
| Z.19 | `PENDING` | `PASS` | `complete` | `just smoke fast` now creates the clean-room team shell, repairs ATM roster truth with `atm teams add-member`, proves both send modes plus recipient read/ack/nudge flow, writes `reports/smoke/smoke-fast.md`, and fails on missing retained-log events or any warning/error records |
| Z.20 | `PENDING` | `PENDING` | `not started` | implement the default `just smoke` lane; close only when the broader retained/admin/operator surface is covered and every deviation carries a root-cause note |
| Z.21 | `PENDING` | `PENDING` | `not started` | implement `just smoke thorough`; close only when every frozen smoke row and every listed CLI happy/common-error-path contract is reported explicitly |
| Z.22 | `PENDING` | `PENDING` | `not started` | establish `docs/phase-Z/smoke-findings-review.md` as the authoritative major-smoke-findings queue with explicit disposition and readiness linkage |
| Z.23 | `PENDING` | `PENDING` | `not started` | add explicit `just test coverage` reporting and tracked latest `reports/coverage/mac.md` / `reports/coverage/win.md` without widening ordinary `just test` |
| Z.3 | `97518da5` | `PASS` | `complete` | `Z.17` executed the `atm-dev` canary on the accepted post-`Z.16` integration baseline with the frozen participant minimum `team-lead + arch-ctm`; retained command, send, read, and log-reporting flows succeeded; no validated canary findings were promoted to `Z.4`. |
| Z.4 | `PENDING` | `PENDING` | `not started` | awaits `Z.3` closure plus the accepted `Z.23` coverage-report line before final release verdict |

## Deferred Follow-Up Findings

- `Z.15`
  - `RSH-001`: daemon join-handle tracking hardening
  - `RSH-002`: daemon runtime shutdown-path hardening
  - `RSH-003`: daemon lifecycle edge-case hardening
  - `RSH-004`: daemon control-path hardening
  - `RSH-005`: daemon signal-path hardening
  - `RSH-006`: daemon worker coordination hardening
  - `RSH-007`: daemon platform/runtime hardening
  - `RSH-009`: local IPC timeout-path tracing hardening
  - `RBP-F001`: typed newtypes for `RosterMemberRecord.agent_type` /
    `.model`
  - `RBP-F002`: dedicated `PaneId` newtype for Claude pane metadata instead
    of the current `Option<String>` surface
  - `RBP-F003`: typed `ResolvedTarget` hardening
  - `RBP-F004`: structured error-diagnostics hardening
  - `RBP-F005`: replayed local IPC path hardening
  - `M-004`: stronger typed/length-constrained
    `AddMemberRequest.agent_type` / `.model`
  - `ATM-QA-Z10-001`: complete the remaining `Z.8` production wiring
    follow-up before final release closeout

Final release verdict:

- integrate/phase-Z candidate: `PENDING`
- release checklist result: `PENDING`
- release verdict: `PENDING`
- authorized by: `PENDING`
- notes: release sign-off not yet recorded
