# Phase BL — atm-bd-orchestration 0.18.5

## Authority, architecture, and plan DAG

Phase BL implements collectors → persisted JSON state → state stage →
EventRequest → debounce/aggregate → separately scheduled delivery. It adds no
database. design.md holds Rand's verbatim authority. The sole machine-readable
DAG is docs/plans/phase-bl/phase-bl.jsonl. bl-9 was deleted before planning
because it duplicated the collector/snapshot seam; its work is now bl-8.
bl-14 is folded into bl-12 composition; no separate bl-14 deliverable remains.

Implementation lands in the separate `randlee/atm-bd-orchestration` repository:
all sprint branches target its `integrate/phase-bl` (cut from `main`) and all
listed validation commands run in that repository's worktree. This `atm-core`
branch (`plan/phase-bl`, PR to `develop`) publishes the contract and dispatch
plan only; it is handed to the implementation repository before Wave 1.

## Pre-Wave-1 handoff

bl-1 owns the handoff gate. Before its implementation branch opens, copy this
plan, `design.md`, `phase-bl.jsonl`, the `beads/` render, and
`.sprints/BL/structure.ttl` into `randlee/atm-bd-orchestration` at
`docs/plans/phase-bl/`; record both the atm-core plan SHA and destination SHA
in `phase-bl-handoff.json`. Dispatch/QA parses the copied JSONL and registry,
resolves every criteria path, and rejects a mismatch. The gate test is
`test_phase_bl_handoff_pins_commits_and_resolves_all_sprints`.

## Boundary and ownership map

| Sprint | Boundary / closure | Exact owned paths (disjoint within wave) |
| --- | --- | --- |
| bl-1 | PhaseBlContract / contract | apps/atm-bd-console/src/atm_bd_console/{types,codec,registry}.py; apps/atm-bd-console/tests/{test_types,test_codec,test_registry}.py |
| bl-2 | JevResult / boundary | plugins/atm-bd-orchestration/{skills/atm-bd-orchestration/scripts/{jev_client.py,wrap_jev_receipt.py,sanity-split},agents/sc-sanity-jev.md,scripts/tests/test_{jev_client,wrap_jev_receipt,sanity_split,jev_client_prompts}.py} |
| bl-3 | EventDispatch / boundary | apps/atm-bd-console/src/atm_bd_console/{events,delivery}.py; apps/atm-bd-console/tests/{test_events,test_delivery}.py; sole `enqueue-event` CLI, sender schedule, and recipient tests |
| bl-4 | QaReviewers / boundary | plugins/atm-bd-orchestration/{roles/quality-mgr.md,templates/qa-template.xml.j2,templates/finding-bead.json.j2,scripts/tests/test_qa_report_contract.py} |
| bl-5 | SanityState / boundary | plugins/atm-bd-orchestration/{agents/dev-sanity.md,templates/dev-sanity-template.xml.j2,templates/sanity-run-record.json.j2,scripts/{sanity-merge,sanity-run-history,sanity-stack-link,atm-task-close.py},scripts/tests/{test_sanity_merge,test_sanity_run_history,test_templates,test_handoff_contracts,test_dev_sanity_prompt}.py} |
| bl-6 | DashboardRender / boundary | apps/atm-bd-console/src/atm_bd_console/{render,status,show}.py; apps/atm-bd-console/tests/test_{render,status,show}.py; apps/atm-bd-console/README.md |
| bl-7 | QualityTab / boundary | apps/atm-bd-console/src/atm_bd_console/{runs,icons}.py; apps/atm-bd-console/tests/test_{runs,icons}.py |
| bl-8 | Collectors / boundary | apps/atm-bd-console/src/atm_bd_console/{refresh,state,adapters/{__init__,bd,atm,github}.py,cli.py,command.py}; matching collector tests |
| bl-10 | ProblemSources / boundary | plugins/atm-bd-orchestration/{atm_bd_orchestration/{github_source,atm_bd,problem_sources,__init__}.py,pyproject.toml,uv.lock,manifest.yaml,tests/gen_manifest.py}; matching tests |
| bl-13 | DagRender / boundary | apps/atm-bd-console/src/atm_bd_console/dag.py; apps/atm-bd-console/tests/test_dag.py |
| bl-12 | PhaseBlComposition / integration | apps/atm-bd-console/src/atm_bd_console/{cli,app,runner,model,cron_entry,delivery_entry,wiring}.py; apps/atm-bd-console/tests/{test_phase_bl_composition,test_app}.py |

## Three waves

| Wave | Sprints | Contract / ordering |
| --- | --- | --- |
| 1 | bl-1 | publishes the complete Python/JSON contract and fixtures |
| 2 | bl-2, bl-3, bl-4, bl-5, bl-6, bl-7, bl-8, bl-10, bl-13 | each consumes only bl-1; paths are disjoint |
| 3 | bl-12 | composes every wave-2 production boundary |

Sprint count is 11, usable width is 9, and critical path is 3
(bl-1 → any wave-2 sprint → bl-12). Every branch is sprint/bl-<id>-<slug>
and targets integrate/phase-bl. This plan intentionally records no synthetic
per-sprint layer: all Wave-2 boundaries are siblings cut from the Wave-1
contract, and the stack writer cuts any delivery stack at landing. The plan
branch alone targets develop.

## Issue disposition

| Issue | Sprint | Deliverable | Deterministic proof / status |
| --- | --- | --- | --- |
| #379 | bl-2 | D6 | `test_pr_body_null_is_empty_text` |
| #164 | bl-2 | D7 | `test_external_evidence_flag_appends` |
| #365, #497 | bl-5 | D1 | `test_sanity_log_preserves_jev_request_array_and_jsonl` |
| #375, #372 | bl-10 | D2 | `test_latest_non_cancelled_check_per_name_wins` |
| #492 | bl-8 | carry-forward | D2 remains open; no proof test is claimed |
| #126, #498 | bl-5 | D3 | `test_not_reproducible_close_and_rebase_link` |
| #533 | bl-8 | D4 | `test_d1_deadline_is_required_before_command_dispatch` |
| #491, #359 | bl-3 | D1 | `test_derive_transitions_emits_held_dispatch_and_alert_requests` |
| #360 | bl-13 | D1 | `test_dag_mmds_fixture_node_ids_labels_and_strokes` |
| #347 | bl-12 (closure); bl-4 producer; bl-7 UI | D3 | `test_composed_quality_two_panel_markdown_selection` |

## Integration exit

bl-12 owns the real cron entry (collect → build_snapshot → enqueue) and
delivery entry (deliver_due) wiring. Its composed tests inject only clocks,
ATM, and source fakes and alone prove cron/TUI equality, no send from
collectors/TUI, retry after source failure, and scheduled delivery.
