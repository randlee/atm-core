# Sprint bl-7 — Quality tab and Jev presentation

closure_type: boundary; target_boundary: QualityTab; branch:
sprint/bl-7-quality-tab; PR target: integrate/phase-bl; wave 2; external_ref:
#347; recommended_agent: trinity; recommended_model: gpt-6.1-sol. must_follow:
bl-1 (shared types); parallel_safe: every other wave-2 sprint.

Own runs.py, icons.py, test_runs.py and test_icons.py. app.py/refresh/timers
are bl-14.

> "on qa page, we should probably create 2 horizontal panels (top/bottom).  top would be ~60% and would have most recent qa run on top.  when selecting a row of a qa run, the lower pane would have a vertical split each qa subagent w/ status (pass/fail Nerd icon), elapsed time of subagent, if subagent was selected,  finding text for all findings would show up in lower right panel which rendered it nicely."

1. Quality table matches Sanity: aligned time/sprint/result/elapsed/dev and
reviewer columns, no vertical bars; selected details filter reviewer findings,
dedupe contributors, mark filer, and show exact never-loaded/metadata text.
2. API_FAILURE is red nf-fa-ban U+F05E, distinct from Jev verdicts; detail
shows typed error and retry count.

- [ ] test_quality_table_columns_match_sanity
- [ ] test_quality_no_vertical_bars
- [ ] test_reviewer_selection_filters_findings
- [ ] test_finding_lists_contributing_reviewers_filing_marked
- [ ] test_missing_reviewers_metadata_placeholder
- [ ] test_jev_api_failure_uses_nf_fa_ban_u_f05e
- [ ] test_jev_detail_shows_typed_error_and_retry_count

It does not own headers, collection requests, locks, or delivery.
