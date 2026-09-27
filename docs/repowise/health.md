# sc-compose — Repowise Code Health Analysis

**Version:** v1.3.1-1709-gcb46830b8 | **Commit:** cb46830b | **Generated:** 2026-08-01
**Analyzed by:** repowise health + dead-code + refactoring-targets

## Quick Summary

| Metric | Value |
|---|---|
| Overall Health | **7.7/10** |
| Hotspot Health | 4.5/10 |
| Worst File | `crates/atm-core/src/observability.rs` (1.7/10) |
| Files Indexed | 1199 |
| Biomarker Findings | 1394 |
| Dead Code Items | 63 |
| Refactoring Targets | 20 |

### Health Dimensions

| Dimension | Average | Hotspot |
|---|---|---|
| Maintainability | 8.7/10 | 7.0/10 |
| Performance | 9.8/10 | 9.4/10 |
| Overall | 7.7/10 | 4.5/10 |

*Interpretation:* 8.2/10 average with 5.2 hotspot health means most files are healthy but a concentrated few drag the score down. Maintainability (9.1) and performance (9.8) average are excellent, but the hotspot maintainability (7.3) reveals files needing modularization. The worst file (`validation.rs`) at 2.6/10 accounts for most of the hotspot drag.

## Worst 20 Files by Health Score

| Score | File | NLOC | CCN | Nest | Dup% |
|---|---|---|---|---|---|
| 1.7 | `crates/atm-core/src/observability.rs` | 850 | 19 | 4 | 27.6 |
| 1.9 | `crates/atm/src/output.rs` | 823 | 27 | 3 | 28.5 |
| 2.2 | `crates/atm-daemon/src/peer_drain_coordinator.rs` | 942 | 12 | 4 | 13.8 |
| 2.3 | `crates/atm-core/src/read/mod.rs` | 1932 | 9 | 4 | 41.2 |
| 2.4 | `crates/atm-daemon/src/local_ipc_transport.rs` | 946 | 10 | 4 | 11.6 |
| 2.4 | `crates/atm-daemon-client/src/lib.rs` | 1318 | 22 | 5 | 13.5 |
| 2.5 | `crates/atm-storage-rusqlite/src/writer/mod.rs` | 513 | 11 | 5 | 1.5 |
| 2.5 | `crates/atm-daemon/src/https_transport.rs` | 1585 | 7 | 4 | 22.5 |
| 2.9 | `crates/atm-storage-rusqlite/src/shared_db.rs` | 906 | 13 | 5 | — |
| 3.0 | `crates/atm-daemon/bin_support/daemon_observability.rs` | 1184 | 8 | 4 | 36.4 |
| 3.4 | `crates/atm-daemon/src/tests/runtime_root/peer_reconciliation.rs` | 728 | 4 | 2 | 29.2 |
| 3.5 | `crates/atm-core/src/mailbox/mod.rs` | 786 | 15 | 4 | 26.0 |
| 3.5 | `crates/atm-architecture/tests/boundary_enforcement.rs` | 1888 | 20 | 3 | 14.8 |
| 3.5 | `crates/atm-core/src/service_runtime_store.rs` | 180 | 4 | 2 | 15.6 |
| 3.7 | `crates/atm-daemon/src/runtime_health.rs` | 1106 | 11 | 2 | 3.6 |
| 3.8 | `crates/atm-core/src/api.rs` | 891 | 15 | 4 | 20.7 |
| 4.0 | `crates/atm-daemon/src/runtime_status_cache.rs` | 537 | 11 | 2 | 10.8 |
| 4.0 | `crates/atm-core/src/send/mod.rs` | 1079 | 6 | 3 | 14.3 |
| 4.1 | `crates/atm-core/src/list.rs` | 605 | 4 | 1 | 56.5 |
| 4.2 | `crates/atm-core/src/types.rs` | 82 | 1 | 0 | 75.0 |

**Key observations:**
- `validation.rs` (2.6/10, 1388 NLOC, CCN=11): the single biggest problem — large, complex, and duplicated (28% dup). This is a prime candidate for decomposition.
- Test files dominate the worst list: `cli.rs` (2586 NLOC, 61% dup), `json_cli.rs` (1640 NLOC, 76% dup) — these are expected for thorough testing but the duplication indicates test helper opportunities.
- `types.rs` (4.8/10, CCN=11, nest=6): deeply nested validation logic — the 6-deep nesting in `validate_input_value_at` is flagged separately.

## Best 10 Files (for contrast)

| Score | File | NLOC |
|---|---|---|
| 10.0 | `crates/atm-storage-sqlserver-proof/Cargo.toml` | 15 |
| 10.0 | `crates/atm-storage-rusqlite/Cargo.toml` | 27 |
| 10.0 | `crates/atm-storage/src/validation.rs` | 87 |
| 10.0 | `crates/atm-storage/Cargo.toml` | 17 |
| 10.0 | `crates/atm-runtime-test-support/Cargo.toml` | 17 |
| 10.0 | `crates/atm-runtime/Cargo.toml` | 17 |
| 10.0 | `crates/atm-graft-python/tests/python_api.rs` | 4 |
| 10.0 | `crates/atm-graft-python/pyproject.toml` | 10 |
| 10.0 | `crates/atm-graft-python/Cargo.toml` | 21 |
| 10.0 | `crates/atm-graft/Cargo.toml` | 21 |

## Biomarker Findings

| Type | Count | What It Means |
|---|---|---|
| duplicated_assertion_block | 130 | Repeated assertion patterns — test helper opportunity |
| hot_path_sync_io | 52 | Sync I/O on hot paths — should be async |
| prior_defect | 46 | Files with bug-fix history — strong defect predictor |
| dry_violation | 38 | DRY violations — opportunities to extract shared code |
| error_handling | 30 | Error handling gaps or inconsistencies |
| hidden_coupling | 21 | Implicit dependencies between modules |
| co_change_scatter | 21 | Files that change together → high coupling |
| primitive_obsession | 113 | |
| low_cohesion | 51 | |
| churn_risk | 48 | |
| change_entropy | 45 | |
| function_hotspot | 33 | |
| complex_method | 28 | |
| large_method | 28 | |
| io_in_loop | 22 | |
| nested_complexity | 22 | |
| untested_hotspot | 8 | |
| knowledge_loss | 2 | |
| complex_conditional | 1 | |
| brain_method | 1 | |
| bumpy_road | 1 | |
| god_class | 1 | |

### prior_defect (178 findings)

- **critical** `crates/atm/src/commands/ack.rs` `(top-level)`: 5 bug-fixes touched this file in the last ~6 months; recent defect history is the strongest cost-effective predictor of further defects
- **critical** `crates/atm/src/commands/teams.rs` `(top-level)`: 20 bug-fixes touched this file in the last ~6 months; recent defect history is the strongest cost-effective predictor of further defects
- **critical** `crates/atm-core/src/address.rs` `(top-level)`: 7 bug-fixes touched this file in the last ~6 months; recent defect history is the strongest cost-effective predictor of further defects
- **critical** `crates/atm-core/src/caller_context.rs` `(top-level)`: 7 bug-fixes touched this file in the last ~6 months; recent defect history is the strongest cost-effective predictor of further defects
- **critical** `crates/atm-core/src/error_codes.rs` `(top-level)`: 13 bug-fixes touched this file in the last ~6 months; recent defect history is the strongest cost-effective predictor of further defects
- **critical** `crates/atm-core/src/persistence.rs` `(top-level)`: 3 bug-fixes touched this file in the last ~6 months; recent defect history is the strongest cost-effective predictor of further defects
- *... and 172 more*

### untested_hotspot (8 findings)

- **critical** `crates/atm/src/commands/caller_context.rs` `(top-level)`: Hotspot with no paired test file and no coverage data — 10 dependents
- **critical** `crates/atm-core/src/error.rs` `(top-level)`: Hotspot with no paired test file and no coverage data — 52 dependents
- **critical** `crates/atm-core/src/service_runtime_store.rs` `(top-level)`: Hotspot with no paired test file and no coverage data — 10 dependents
- **critical** `crates/atm-core/src/types.rs` `(top-level)`: Hotspot with no paired test file and no coverage data — 53 dependents
- **high** `crates/atm-core/src/delivery_plan.rs` `(top-level)`: Hotspot with no paired test file and no coverage data — 4 dependents
- **high** `crates/atm-core/src/config/types.rs` `(top-level)`: Hotspot with no paired test file and no coverage data — 6 dependents
- *... and 2 more*

### co_change_scatter (102 findings)

- **high** `crates/atm-core/src/config/aliases.rs` `(top-level)`: co-changes with 21 distinct files — editing this file tends to ripple across the codebase (shotgun surgery)
- **high** `crates/atm-core/src/identity/hook.rs` `(top-level)`: co-changes with 22 distinct files — editing this file tends to ripple across the codebase (shotgun surgery)
- **high** `crates/atm-core/src/team_admin/member_mutation.rs` `(top-level)`: co-changes with 27 distinct files — editing this file tends to ripple across the codebase (shotgun surgery)
- **high** `crates/atm-storage/src/types.rs` `(top-level)`: co-changes with 19 distinct files — editing this file tends to ripple across the codebase (shotgun surgery)
- **high** `crates/atm-storage/src/schema/inbox_message.rs` `(top-level)`: co-changes with 20 distinct files — editing this file tends to ripple across the codebase (shotgun surgery)
- **high** `crates/atm-core/src/send/file_policy.rs` `(top-level)`: co-changes with 16 distinct files — editing this file tends to ripple across the codebase (shotgun surgery)
- *... and 96 more*

### churn_risk (48 findings)

- **critical** `crates/atm/src/commands/caller_context.rs` `(top-level)`: 90-day churn rewrote 86.4x the file's size (432 lines over 5 NLOC, top 22% of repo churn)
- **critical** `crates/atm-daemon/src/daemon_observability.rs` `(top-level)`: 90-day churn rewrote 2621.0x the file's size (2621 lines over 1 NLOC, top 6% of repo churn)
- **critical** `crates/atm-core/src/types.rs` `(top-level)`: 90-day churn rewrote 13.6x the file's size (1116 lines over 82 NLOC, top 6% of repo churn)
- **critical** `crates/atm-core/src/send/graft_warning_tests.rs` `(top-level)`: 90-day churn rewrote 5.3x the file's size (448 lines over 85 NLOC, top 22% of repo churn)
- **critical** `crates/atm-core/src/error.rs` `(top-level)`: 90-day churn rewrote 846.0x the file's size (846 lines over 1 NLOC, top 7% of repo churn)
- **critical** `crates/atm-core/src/service_runtime_store.rs` `(top-level)`: 90-day churn rewrote 16.2x the file's size (2914 lines over 180 NLOC, top 4% of repo churn)
- *... and 42 more*

### hidden_coupling (147 findings)

- **critical** `crates/atm-core/src/doctor/report.rs` `(top-level)`: crates/atm-daemon/src/runtime_health.rs co-changes with this file 10 times (200% of shared commits) but no static dependency exists
- **critical** `crates/atm/src/commands/members.rs` `(top-level)`: crates/atm/src/composition.rs co-changes with this file 10 times (143% of shared commits) but no static dependency exists
- **critical** `crates/atm-core/src/team_admin/restore.rs` `(top-level)`: crates/atm-core/src/doctor/mod.rs co-changes with this file 10 times (143% of shared commits) but no static dependency exists
- **critical** `crates/atm-runtime/src/legacy_storage_adapters.rs` `(top-level)`: crates/atm-storage-rusqlite/src/lib.rs co-changes with this file 9 times (150% of shared commits) but no static dependency exists
- **critical** `crates/atm-core/src/config/mod.rs` `(top-level)`: crates/atm/src/composition.rs co-changes with this file 8 times (114% of shared commits) but no static dependency exists
- **critical** `crates/atm-daemon/src/tests.rs` `(top-level)`: crates/atm-daemon/src/tests/runtime_root.rs co-changes with this file 12 times (200% of shared commits) but no static dependency exists
- *... and 141 more*

### error_handling (236 findings)

- **low** `crates/atm/src/main.rs` `(top-level)`: unwrap/expect turns a recoverable error into a crash
- **low** `crates/atm/src/main.rs` `(top-level)`: unwrap/expect turns a recoverable error into a crash
- **low** `crates/atm/src/bin/atm_post_send_hook_fixture.rs` `(top-level)`: unwrap/expect turns a recoverable error into a crash
- **low** `crates/atm/src/commands/api.rs` `(top-level)`: panic!/unreachable!/todo!/unimplemented! aborts the process unconditionally
- **low** `crates/atm/src/commands/peer.rs` `(top-level)`: unwrap/expect turns a recoverable error into a crash
- **low** `crates/atm/src/commands/send.rs` `(top-level)`: panic!/unreachable!/todo!/unimplemented! aborts the process unconditionally
- *... and 230 more*

## Refactoring Targets

Prioritized by impact-per-effort ratio (highest ROI first).

### #1: `crates/atm/src/commands/caller_context.rs` (4.5/10, 5 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **untested_hotspot** (critical) |
| Impact Score | 5.5 |
| Effort | S |
| ROI | 5.5 |
| Finding Count | 3 |
| Reason | Hotspot with no paired test file and no coverage data — 10 dependents |

### #2: `crates/atm-core/src/error.rs` (4.5/10, 1 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **untested_hotspot** (critical) |
| Impact Score | 5.5 |
| Effort | S |
| ROI | 5.5 |
| Finding Count | 3 |
| Reason | Hotspot with no paired test file and no coverage data — 55 dependents |

### #3: `crates/atm-daemon/src/daemon_observability.rs` (6.5/10, 1 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **churn_risk** (critical) |
| Impact Score | 3.5 |
| Effort | S |
| ROI | 3.5 |
| Finding Count | 2 |
| Reason | 90-day churn rewrote 2621.0x the file's size (2621 lines over 1 NLOC, top 6% of repo churn) |

### #4: `crates/atm-core/src/boundary_support.rs` (6.5/10, 19 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **churn_risk** (critical) |
| Impact Score | 3.5 |
| Effort | S |
| ROI | 3.5 |
| Finding Count | 3 |
| Reason | 90-day churn rewrote 144.8x the file's size (2752 lines over 19 NLOC, top 2% of repo churn) |

### #5: `crates/atm-runtime/src/lib.rs` (6.5/10, 12 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **co_change_scatter** (high) |
| Impact Score | 3.5 |
| Effort | S |
| ROI | 3.5 |
| Finding Count | 4 |
| Reason | co-changes with 52 distinct files — editing this file tends to ripple across the codebase (shotgun surgery) |

### #6: `crates/atm-core/src/types.rs` (4.2/10, 82 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **untested_hotspot** (critical) |
| Impact Score | 5.8 |
| Effort | M |
| ROI | 2.9 |
| Finding Count | 4 |
| Reason | Hotspot with no paired test file and no coverage data — 55 dependents |

### #7: `crates/atm-runtime-test-support/src/lib.rs` (4.6/10, 87 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **untested_hotspot** (high) |
| Impact Score | 5.4 |
| Effort | M |
| ROI | 2.7 |
| Finding Count | 6 |
| Reason | Hotspot with no paired test file and no coverage data — 9 dependents |

### #8: `.claude/skills/graph-orchestration/scripts/validate-findings.py` (2.0/10, 376 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **prior_defect** (critical) |
| Impact Score | 8.0 |
| Effort | L |
| ROI | 2.7 |
| Finding Count | 15 |
| Reason | 3 bug-fixes touched this file in the last ~6 months; recent defect history is the strongest cost-effective predictor of further defects |

- **extract_method**: {'span': {'start': 185, 'end': 205}, 'params': ['finding', 'finding_files', 'finding_graph', 'parsed', 'path'], 'returns': ['linked_records'], 'sugges
- **extract_method**: {'span': {'start': 333, 'end': 354}, 'params': ['events', 'graph', 'known_sprints', 'script_dir', 'structure'], 'returns': ['rows'], 'suggested_name':

### #9: `.claude/skills/daemon-switch/scripts/daemon-switch.py` (2.2/10, 353 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **prior_defect** (critical) |
| Impact Score | 7.8 |
| Effort | L |
| ROI | 2.6 |
| Finding Count | 12 |
| Reason | 7 bug-fixes touched this file in the last ~6 months; recent defect history is the strongest cost-effective predictor of further defects |

- **extract_method**: {'span': {'start': 276, 'end': 281}, 'params': ['args', 'cli_link', 'daemon_link', 'old_pair'], 'returns': [], 'suggested_name': None}

### #10: `crates/atm-storage/src/lib.rs` (7.4/10, 28 NLOC)

| Metric | Value |
|---|---|
| Biomarker | **prior_defect** (critical) |
| Impact Score | 2.6 |
| Effort | S |
| ROI | 2.6 |
| Finding Count | 2 |
| Reason | 13 bug-fixes touched this file in the last ~6 months; recent defect history is the strongest cost-effective predictor of further defects |

## Dead Code Analysis

**Note:** 51 `unused_export` findings in `bindings/python/python/sc_compose/_native.pyi` are PyO3 auto-generated type stubs — not genuine dead code. They are excluded from the actionable count below.

| Kind | Total | Actionable | Action |
|---|---|---|---|
| unreachable_file | 50 | 50 | Review — may be dead or scripts/prototypes |
| unused_export | 12 | 12 | 12 clean-up candidates |
| zombie_package | 1 | 1 | Review prototype/ package |

### Unreachable Files

- `.claude/scripts/worktree_abort.py` (50 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/scripts/worktree_cleanup.py` (80 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/scripts/worktree_create.py` (50 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/scripts/worktree_scan.py` (70 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/scripts/worktree_update.py` (90 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/skills/daemon-switch/scripts/daemon-switch.py` (280 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/skills/graph-orchestration/scripts/assignee-busy` (0 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/skills/graph-orchestration/scripts/check_assignee.py` (20 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/skills/graph-orchestration/scripts/next-dev-task` (0 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/skills/graph-orchestration/scripts/preflight` (0 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/skills/graph-orchestration/scripts/preflight.py` (130 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/skills/graph-orchestration/scripts/validate-findings.py` (190 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/skills/triage-report/scripts/triage_report.py` (380 lines) — File has no importers (in_degree=0) [risks: script]
- `.claude/skills/triaging-findings/scripts/check_dependencies.py` (90 lines) — File has no importers (in_degree=0) [risks: script]
- `.just/lint_same_host_portability.py` (150 lines) — File has no importers (in_degree=0) [risks: none]
- `.just/run_tests.py` (30 lines) — File has no importers (in_degree=0) [risks: none]
- `crates/atm-core/src/api/http_frame_reader.rs` (600 lines) — File has no importers (in_degree=0) [risks: none]
- `crates/atm-daemon/bin_support/daemon_observability.rs` (1030 lines) — File has no importers (in_degree=0) [risks: none]
- `crates/atm-daemon/src/local_ipc_transport/accept_loop.rs` (20 lines) — File has no importers (in_degree=0) [risks: none]
- `crates/atm-daemon/src/runtime_health/admission_view.rs` (80 lines) — File has no importers (in_degree=0) [risks: none]
- `crates/atm-daemon/src/runtime_health/doctor_reporting.rs` (40 lines) — File has no importers (in_degree=0) [risks: none]
- `crates/atm-daemon/src/runtime_health/peer_delivery_router.rs` (40 lines) — File has no importers (in_degree=0) [risks: none]
- `crates/atm-daemon/src/runtime_health/post_commit_work.rs` (410 lines) — File has no importers (in_degree=0) [risks: none]
- `crates/sc-lint-boundary/src/graph/ingest.rs` (540 lines) — File has no importers (in_degree=0) [risks: none]
- `crates/sc-lint-boundary/src/graph/reference_collector.rs` (130 lines) — File has no importers (in_degree=0) [risks: none]
- `docs/reports/generate_diagram_pages.py` (310 lines) — File has no importers (in_degree=0) [risks: none]
- `docs/reports/assets/diagram-panels.js` (10 lines) — File has no importers (in_degree=0) [risks: none]
- `scripts/atm-nudge.py` (220 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/check-capability-degradation.py` (60 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/check-function-length.py` (200 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/check-legacy-mailbox-paths.py` (90 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/check-silent-emit.py` (70 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/claude_inbox_send.py` (40 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/find_todos.py` (110 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/triage_carry_forward.py` (70 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/validate_release.py` (390 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/verify_release_archive.py` (40 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/verify_user_docs.py` (180 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/fuzz/run_local_http_framing_campaign.py` (230 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/phase-ai/run-hermes-graft-smoke.py` (30 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/phase-ai/run-hermes-steer-smoke.py` (50 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/smoke/combine_inbound_peer_smoke.py` (90 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/smoke/render_report.py` (40 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/smoke/run_admission_capacity.py` (540 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/smoke/run_feature_smoke.py` (450 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/smoke/run_graft_same_host.py` (120 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/smoke/run_peer_pair.py` (310 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/smoke/run_thorough.py` (40 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/smoke/run_thorough_retry.py` (10 lines) — File has no importers (in_degree=0) [risks: script]
- `scripts/smoke/run_thorough_shared_host.py` (130 lines) — File has no importers (in_degree=0) [risks: script]

### Actionable Unused Exports (excl. PyO3 stubs)

- `.claude/scripts/worktree_scan.py`: `WorktreeStatus` — Public symbol 'WorktreeStatus' has no importers
- `.claude/scripts/worktree_shared.py`: `validate_allowed_path` — Public symbol 'validate_allowed_path' has no importers
- `.claude/scripts/worktree_shared.py`: `load_runtime_context` — Public symbol 'load_runtime_context' has no importers
- `.claude/scripts/worktree_shared.py`: `find_repo_root` — Public symbol 'find_repo_root' has no importers
- `.claude/scripts/worktree_shared.py`: `is_git_repo` — Public symbol 'is_git_repo' has no importers
- `.claude/scripts/worktree_shared.py`: `validate_hook_json` — Public symbol 'validate_hook_json' has no importers
- `.claude/scripts/worktree_shared.py`: `invoke_agent_runner` — Public symbol 'invoke_agent_runner' has no importers
- `.claude/scripts/worktree_shared.py`: `sync_tracking_with_remote` — Public symbol 'sync_tracking_with_remote' has no importers
- `.just/check_env_var_boundary.py`: `find_function_definition_line` — Public symbol 'find_function_definition_line' has no importers
- `scripts/atm-nudge.py`: `emit_hook_result` — Public symbol 'emit_hook_result' has no importers
- *... and 2 more*

## Top Recommendations

### 1. Decompose `validation.rs` (2.6/10, 1388 NLOC)
The largest and worst-scoring file. It has 20 biomarker findings, CCN=11, 28% duplication, and co-changes with 24 other files. Split into per-category validators: `var_validation.rs`, `frontmatter_validation.rs`, `include_validation.rs`.

### 2. Add tests for untested hotspots
11 files flagged as untested hotspots — `path_utils.rs` (16 dependents), `diagnostics.rs` (13 dependents), `cli.rs` (7 dependents). These are heavily depended-upon files with no paired test coverage. Prioritize `path_utils.rs` first (critical severity, highest ROI refactoring target).

### 3. Extract test helpers (`cli.rs` 61% dup, `json_cli.rs` 76% dup)
130 duplicated assertion blocks in test files — extract shared assertion helpers. The high duplication percentage in test files is expected but the volume (130 findings) signals a real maintenance burden.

### 4. Address sync I/O on hot paths (52 findings)
52 hot path sync I/O findings — likely from file I/O in the rendering pipeline. Consider async or at minimum document the sync I/O is intentional for CLI tools.

### 5. Review prototype/ package visibility
The `prototype/` directory is flagged as a zombie package. If actively used for experimentation, add to the repowise config's `annotated` section. Otherwise, consider archiving.

---
*Generated by repowise v1.x — codebase intelligence for developers. Config: .sc/repowise.yaml with modules `crates/sc-compose`, `crates/sc-composer` and annotated paths `bindings/python`, `prototype`, `scripts`.*