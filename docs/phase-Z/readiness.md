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
| Z.2 | `PENDING` | `PENDING` | `not started` | awaits `Z.1` closure |
| Z.5 | `4531eaea` | `PASS` | `complete` | retained command membership checks now use ATM roster truth only; doctor drift warnings compare Claude file members against ATM roster truth; `cargo test --workspace` PASS; `cargo fmt --all --check` PASS; production grep gate leaves only doctor compare-only and test-only `load_team_config(...)` matches; `ack_mail_uses_atm_roster_truth_for_valid_actor` PASS |
| Z.6 | `3cd6fb74` | `PASS` | `complete` | Claude send no longer uses pre-write `config.json` membership gating; immutable `ClaudeCodeTeamRoster` now drives the post-write warning path from store-backed ATM roster rows; `cargo test --workspace` PASS; targeted `z6_post_write_warning_uses_store_backed_claude_roster` PASS; production grep gate leaves no `load_team_config(...)` matches in `send` or `service_runtime`; `cargo fmt --all --check` PASS; `git diff --check` PASS |
| Z.7 | `1fd6ee1e` | `PASS` | `complete` | `ConfigIngress` no longer exposes generic team-config lookup; daemon/core forwarding chain removed; startup-only `hydrate_roster_from_team_config_once_at_startup_if_empty(...)` allowlisted through `Z.8` and proven to no-op when roster state is non-empty; `just lint boundaries` PASS with fixture self-test; `cargo test --workspace` PASS; `cargo fmt --all --check` PASS; `git diff --check` PASS |
| Z.8 | `5040aa19` | `PASS` | `complete` | watcher / reconcile now owns external Claude `config.json` ingest; startup-only bootstrap helper deleted; process-local projection-write suppression skips one matching daemon-authored event and clears on restart; `cargo test --workspace` PASS; targeted `z8_watcher_ingest_hydrates_atm_roster_truth_for_new_team` PASS; targeted `z8_projection_write_suppression_is_process_local` PASS; targeted `z8_deletes_startup_only_config_bootstrap_helper` PASS; `just lint boundaries` PASS; `cargo fmt --all --check` PASS; `git diff --check` PASS |
| Z.9 | `PENDING` | `PENDING` | `not started` | awaits `Z.8` closure |
| Z.10 | `PENDING` | `PENDING` | `not started` | awaits `Z.9` closure |
| Z.3 | `PENDING` | `PENDING` | `not started` | awaits `Z.10` closure |
| Z.4 | `PENDING` | `PENDING` | `not started` | awaits `Z.3` closure |

Final release verdict:

- integrate/phase-Z candidate: `PENDING`
- release checklist result: `PENDING`
- release verdict: `PENDING`
- authorized by: `PENDING`
- notes: release sign-off not yet recorded
