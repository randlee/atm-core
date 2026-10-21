# Sprint bl-10 — problem-source derivation

closure_type: boundary; target_boundary: ProblemSources; branch: sprint/bl-10-problem-sources; PR target: integrate/phase-bl; wave: 2; external_ref: #375, #372; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (versioned collector-record fixtures and `ActionableProblem` codec); parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-6, bl-7, bl-8, bl-13, bl-14.

Own only `plugins/atm-bd-orchestration/atm_bd_orchestration/{github_source,atm_bd,problem_sources}.py` and matching tests. bl-8 is the sole source-read owner; this sprint is pure derivation over persisted codec fixtures and never calls GitHub, BD, ATM, or delivery.

## Governing text (verbatim)

> "multiple collectors each in separate module governed by it's own rules (i.e. gh rate limited, dolt timeouts, atm no limits today). state evaluation based on current state. specific state transitions would trigger 'atm message request'."

> "if you don't have valid data, don't force an error."

## Deliverables

1. Implement `derive_problem_records(records) -> tuple[ActionableProblem, ...]`, a pure decoder/deriver over bl-1 JSON fixtures. bl-8 passes it into `build_snapshot`; therefore `StateSnapshot.problems` is populated from failed/superseded-check fixtures without a plugin-to-console implementation import.
2. Preserve latest non-cancelled-check selection, direct-branch then sprint-container fallback, sprint labels, and `--exclude-label anchor` in pure `problem_sources.py` helpers.
3. Preserve typed source failure as source status, never an invented actionable problem.

## Deterministic acceptance

- [ ] `test_latest_non_cancelled_check_per_name_wins`
- [ ] `test_branch_falls_back_from_problem_to_sprint_container`
- [ ] `test_sprint_labels_and_anchor_exclusion_are_preserved`
- [ ] `test_failed_source_decodes_to_typed_status_not_forced_problem`
- [ ] `test_derive_problem_records_is_pure_over_contract_fixture`

Use fixture JSON and pure functions only; run plugin pytest/lint with no network, clock, or source reads.

## This sprint does not close

It does not collect source data (bl-8), queue or deliver events (bl-3), or render rows. bl-12 owns integrated source-to-display proof.
