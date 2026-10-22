# Sprint bl-7 — Quality tab and Jev presentation
Implementation repository/worktree: `randlee/atm-bd-orchestration`; branch/PR target: `integrate/phase-bl`.

closure_type: boundary; target_boundary: QualityTab; branch: sprint/bl-7-quality-tab; PR target: integrate/phase-bl; wave: 2; external_ref: #347; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`StateSnapshot` and typed error fixture); parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-6, bl-8, bl-10, bl-13.

## Scope boundary

Own `runs.py`, `icons.py`, and their tests. It presents persisted Quality/Jev data; application locking and composed layout are bl-12 and delivery is not in scope.

## Authoritative deliverables

> "on qa page, we should probably create 2 horizontal panels (top/bottom).  top would be ~60% and would have most recent qa run on top.  when selecting a row of a qa run, the lower pane would have a vertical split each qa subagent w/ status (pass/fail Nerd icon), elapsed time of subagent, if subagent was selected,  finding text for all findings would show up in lower right panel which rendered it nicely."

1. Keep the aligned Quality/Sanity table: time, sprint, result, elapsed, dev, reviewer status; preserve reviewer selection, contributors, filing reviewer, typed details, and `nf-fa-ban` API failure glyph.
2. Render the never-loaded text exactly `Open QA runs: not loaded yet`; distinguish a Sanity API failure from an ordinary `no` result and from absent metadata.
3. Implement #347's 60/40 two-panel layout: the most-recent QA run is the upper pane; selecting a run exposes reviewer status at lower left and rendered Markdown findings at lower right.

## Authoritative acceptance criteria

- [ ] `test_quality_table_columns_match_sanity`
- [ ] `test_quality_never_loaded_placeholder`
- [ ] `test_sanity_row_api_failure_vs_no`
- [ ] `test_missing_reviewers_metadata_placeholder`
- [ ] `test_jev_api_failure_uses_nf_fa_ban_u_f05e`
- [ ] `test_reviewer_selection_filters_findings`
- [ ] `test_finding_lists_contributing_reviewers_filing_marked`
- [ ] `test_quality_two_panel_60_40_layout`
- [ ] `test_quality_selection_renders_markdown_findings_in_lower_right`
- [ ] `test_quality_no_vertical_bars`
- [ ] `test_jev_detail_shows_typed_error_and_retry_count`

## Authoritative validation
```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q tests/test_runs.py tests/test_icons.py
```

## Non-closure

It presents persisted Quality/Jev data; bl-4 produces reviewer metadata, bl-3 delivers events, and bl-12 owns application composition and locking.
