# Sprint bl-7 — Quality tab and Jev presentation

closure_type: boundary; target_boundary: QualityTab; branch: sprint/bl-7-quality-tab; PR target: integrate/phase-bl; wave: 2; external_ref: none; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`StateSnapshot` and typed error fixture); parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-6, bl-8, bl-10, bl-13, bl-14.

Own `runs.py`, `icons.py`, and their tests. It presents persisted Quality/Jev data; app locking is bl-14 and delivery is not in scope.

## Deliverables

1. Keep the aligned Quality/Sanity table: time, sprint, result, elapsed, dev, reviewer status; preserve reviewer selection, contributors, filing reviewer, typed details, and `nf-fa-ban` API failure glyph.
2. Render the never-loaded text exactly `Open QA runs: not loaded yet`; distinguish a Sanity API failure from an ordinary `no` result and from absent metadata.

## Deterministic acceptance

- [ ] `test_quality_table_columns_match_sanity`
- [ ] `test_quality_never_loaded_placeholder`
- [ ] `test_sanity_row_api_failure_vs_no`
- [ ] `test_missing_reviewers_metadata_placeholder`
- [ ] `test_jev_api_failure_uses_nf_fa_ban_u_f05e`
- [ ] `test_reviewer_selection_filters_findings`
- [ ] `test_finding_lists_contributing_reviewers_filing_marked`
