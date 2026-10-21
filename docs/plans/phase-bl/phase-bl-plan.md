# Phase BL — atm-bd-orchestration 0.18.5

## Authority, architecture, and plan DAG

Phase BL implements collectors → persisted JSON state → state stage →
EventRequest → debounce/aggregate → separately scheduled delivery. It adds no
database. design.md holds Rand's verbatim authority. The sole machine-readable
DAG is docs/plans/phase-bl/phase-bl.jsonl. bl-9 was deleted before planning
because it duplicated the collector/snapshot seam; its work is now bl-8.

## Boundary and ownership map

| Sprint | Boundary / closure | Exact owned paths (disjoint within wave) |
| --- | --- | --- |
| bl-1 | PhaseBlContract / contract | apps/atm-bd-console/src/atm_bd_console/{types,codec,registry}.py; apps/atm-bd-console/tests/{test_types,test_codec,test_registry}.py |
| bl-2 | JevResult / boundary | plugins/atm-bd-orchestration/skills/atm-bd-orchestration/scripts/jev_client.py; plugins/atm-bd-orchestration/scripts/tests/test_jev_client.py |
| bl-3 | EventDispatch / boundary | apps/atm-bd-console/src/atm_bd_console/{events,delivery}.py; apps/atm-bd-console/tests/{test_events,test_delivery}.py |
| bl-4 | QaReviewers / boundary | plugins/atm-bd-orchestration/{roles/quality-mgr.md,templates/qa-template.xml.j2,templates/finding-bead.json.j2,scripts/tests/test_qa_report_contract.py} |
| bl-5 | SanityState / boundary | plugins/atm-bd-orchestration/{agents/dev-sanity.md,templates/dev-sanity-template.xml.j2,templates/sanity-run-record.json.j2,scripts/{sanity-merge,sanity-run-history,sanity-stack-link,atm-task-close.py},scripts/tests/test_sanity_*.py} |
| bl-6 | DashboardRender / boundary | apps/atm-bd-console/src/atm_bd_console/{render,status,show}.py; apps/atm-bd-console/tests/test_{render,status,show}.py; apps/atm-bd-console/README.md |
| bl-7 | QualityTab / boundary | apps/atm-bd-console/src/atm_bd_console/{runs,icons}.py; apps/atm-bd-console/tests/test_{runs,icons}.py |
| bl-8 | Collectors / boundary | apps/atm-bd-console/src/atm_bd_console/{refresh,state,adapters/{bd,atm,github}.py,cli.py,command.py}; matching collector tests |
| bl-10 | ProblemSources / boundary | plugins/atm-bd-orchestration/atm_bd_orchestration/{github_source,atm_bd,problem_sources}.py; matching tests |
| bl-13 | DagRender / boundary | apps/atm-bd-console/src/atm_bd_console/dag.py; apps/atm-bd-console/tests/test_dag.py |
| bl-14 | ConsoleApp / boundary | apps/atm-bd-console/src/atm_bd_console/app.py; apps/atm-bd-console/tests/test_app.py |
| bl-12 | PhaseBlComposition / integration | apps/atm-bd-console/src/atm_bd_console/{cron_entry,delivery_entry,wiring}.py; apps/atm-bd-console/tests/test_phase_bl_composition.py |

## Three waves

| Wave | Sprints | Contract / ordering |
| --- | --- | --- |
| 1 | bl-1 | publishes the complete Python/JSON contract and fixtures |
| 2 | bl-2, bl-3, bl-4, bl-5, bl-6, bl-7, bl-8, bl-10, bl-13, bl-14 | each consumes only bl-1; paths are disjoint |
| 3 | bl-12 | composes every wave-2 production boundary |

Sprint count is 12, usable width is 10, and critical path is 3
(bl-1 → any wave-2 sprint → bl-12). Every branch is sprint/bl-<id>-<slug>
and targets integrate/phase-bl; planned layer positions are unique: bl-1 L1;
wave-2 layers L2–L11 in numeric order; bl-12 L12. The plan branch alone
targets develop.

## Issue disposition

| Issue | Sprint | Deliverable | Deterministic proof / status |
| --- | --- | --- | --- |
| #379, #164 | bl-2 | D1 | test_jev_request_array_and_closed_error_enum |
| #365, #497 | bl-5 | D1 | test_sanity_log_preserves_jev_request_array_and_jsonl |
| #375, #372 | bl-10 | D1 | test_latest_non_cancelled_check_and_branch_fallback |
| #492 | bl-8 | carry-forward | D2 remains open; dispatch awaits its ruling |
| #126, #498 | bl-5 | D3 | test_not_reproducible_close_and_rebase_link |
| #533 | bl-8 | D4 | test_d1_deadline_is_required_before_command_dispatch |
| #491, #359 | bl-3 | D2 | test_debounce_and_scheduled_delivery |
| #360 | bl-13 | D1 | test_dag_ansi_fixture_converts_to_rich_text |
| #347 | bl-4/bl-7 | D1/D1 | test_qa_close_step_writes_reviewers / test_quality_table_columns_match_sanity |

## Integration exit

bl-12 owns the real cron entry (collect → build_snapshot → enqueue) and
delivery entry (deliver_due) wiring. Its composed tests inject only clocks,
ATM, and source fakes and alone prove cron/TUI equality, no send from
collectors/TUI, retry after source failure, and scheduled delivery.
