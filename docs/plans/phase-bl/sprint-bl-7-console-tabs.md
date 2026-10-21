# Sprint bl-7 — TUI collectors and persisted-state tabs

`BOUNDARY-ConsoleApp`; wave 2; consumes bl-1 collector records and bl-4
reviewer metadata. The TUI retains a console-instance lock and can request
individual collectors, but displays current persisted state whether cron or
TUI supplied it; it is not a global scheduler and never sends ATM messages.
PR activation requests GitHub; `r` requests collectors subject to their locks.

Keep the Quality two-pane reviewer/finding view, Jev API-failure do-not-enter
icon, typed/never-loaded placeholders, and Diagnostics colour. Deterministic
Textual tests inject collector/persisted records without sleeps or delivery.

## Scope and owned paths

Own `app.py`, `icons.py`, `runs.py` and app/tab/icon/run tests. It consumes
bl-1 records and bl-4 reviewers, not a shared scheduler or delivery seam.

## Authoritative deliverables

1. Console-instance lock; `r` requests every collector, PR activation requests
   GitHub, and every tab renders persisted aggregate state without sending ATM.
2. Status header prints full typed error text and age; Quality has a labelled
   never-loaded row.
3. Quality matches the Sanity tab's table discipline: one aligned line per
   run, no vertical-bar separators, with columns time, sprint, result icon,
   elapsed, dev name and icon, then one status-icon-and-elapsed column for
   every reviewer. Beneath it, whitespace separates the selected run detail:
   selectable reviewers are left and findings Markdown is right. Selecting a
   reviewer filters to that reviewer's findings. A shared finding renders once
   with every contributor and its filing reviewer marked. The never-loaded
   row is exactly `Open QA runs: not loaded yet`; absent metadata says exactly
   `reviewers: not recorded`.
4. `API_FAILURE` uses a red circle with a diagonal line, candidate
   `nf-fa-ban` U+F05E, verified against the Nerd Fonts list and cited in the
   PR. A Jev answer keeps its existing red/green Jev icon. The selected-row
   detail includes the typed error and retry count.
5. `r` requests every collector past its due check while respecting each
   collector lock. A successful collector restarts only its own poll timer; a
   failed collector remains due. If a run already owns a collector lock, `r`
   waits for that result and makes no duplicate query. Activating the PR tab
   requests GitHub only.
6. Diagnostics preserves Rich `Text` colour.

## Deterministic acceptance and validation

Keep old test coverage where behaviour is unchanged, but rename the replaced
Quality-layout tests for the table contract below. Inject collector and
persisted-state records with controlled fakes; no sleeps, threads, delivery,
or timing-based tests. Run frozen ruff, pyright and pytest.

## Governing text (verbatim)

> "on qa page, we should probably create 2 horizontal panels (top/bottom).  top would be ~60% and would have most recent qa run on top.  when selecting a row of a qa run, the lower pane would have a vertical split each qa subagent w/ status (pass/fail Nerd icon), elapsed time of subagent, if subagent was selected,  finding text for all findings would show up in lower right panel which rendered it nicely."

> "if you don't have valid data, don't force an error."

The Quality selected-run detail preserves reviewer selection, findings
Markdown, all contributors, and the filing reviewer marker; its table is now
the Sanity-parity layout above. Verify `nf-fa-ban` U+F05E against the Nerd
Fonts list and cite that verification in the PR.

### Acceptance checklist

- [ ] `test_r_forces_all_sources_pr_tab_forces_github`: requests are scoped.
- [ ] `test_second_console_exits`: console lock rejects another instance.
- [ ] `test_console_never_sends_atm_message`: TUI performs no delivery.
- [ ] `test_status_header_full_typed_text`: status renders typed detail.
- [ ] `test_quality_never_loaded_placeholder`: Quality renders `Open QA runs: not loaded yet`.
- [ ] `test_quality_table_columns_match_sanity`: Quality uses the ordered aligned Sanity-parity columns.
- [ ] `test_quality_no_vertical_bars`: Quality table output has no vertical-bar separators.
- [ ] `test_reviewer_selection_filters_findings`: selection filters findings.
- [ ] `test_finding_lists_contributing_reviewers_filing_marked`: contributors/filer render.
- [ ] `test_missing_reviewers_metadata_placeholder`: absent metadata is labelled.
- [ ] `test_jev_api_failure_uses_nf_fa_ban_u_f05e`: API failure is the red ban glyph and differs from Jev verdicts.
- [ ] `test_jev_detail_shows_typed_error_and_retry_count`: selected-row detail preserves the typed error and retry count.
- [ ] `test_sanity_row_api_failure_vs_no`: API failure differs from Jev no.
- [ ] `test_refresh_key_forces_every_collector_past_due_gate`: `r` bypasses due checks for every collector.
- [ ] `test_refresh_success_restarts_only_its_poll_timer`: success resets only that collector's timer.
- [ ] `test_refresh_failure_stays_due`: a failed forced refresh remains due.
- [ ] `test_refresh_lock_waits_for_existing_result_without_second_query`: an occupied lock is awaited without a duplicate request.
- [ ] `test_pr_tab_activation_requests_github_only`: PR activation does not request other collectors.
- [ ] `test_diagnostics_agent_icons_coloured`: Diagnostics icons retain colour.

## Non-closure

Collection implementation, state evaluation, aggregation, and delivery remain
their producer sprints.
