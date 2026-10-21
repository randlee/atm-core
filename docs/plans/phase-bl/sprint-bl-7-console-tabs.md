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
3. Quality has two panes: newest runs above; reviewer rows below with status,
   elapsed, API_FAILURE icon, reviewer filtering, contributing reviewers and
   filing reviewer marked; absent metadata says `reviewers: not recorded`.
4. `API_FAILURE` uses a verified Nerd Font do-not-enter codepoint distinct
   from a Jev negative verdict, with typed error detail.
5. Diagnostics preserves Rich `Text` colour.

## Deterministic acceptance and validation

Keep old test names for unchanged Quality split/filter/contributor, missing
reviewer, status, icon and Diagnostics behaviour. Add injected collector
requests/persisted-state tests. Run frozen ruff, pyright and pytest; no
sleeps, threads, or message delivery.

## Governing text (verbatim)

> "on qa page, we should probably create 2 horizontal panels (top/bottom).  top would be ~60% and would have most recent qa run on top.  when selecting a row of a qa run, the lower pane would have a vertical split each qa subagent w/ status (pass/fail Nerd icon), elapsed time of subagent, if subagent was selected,  finding text for all findings would show up in lower right panel which rendered it nicely."

> "if you don't have valid data, don't force an error."

The top pane is about 60%; lower left is reviewer rows and lower right is
findings Markdown, including all contributors and filing reviewer marked. The
candidate do-not-enter glyph is `nf-md-minus_circle` U+F0376; verify it
against the Nerd Fonts list and cite the verification in the PR.

### Acceptance checklist

- [ ] `test_r_forces_all_sources_pr_tab_forces_github`: requests are scoped.
- [ ] `test_second_console_exits`: console lock rejects another instance.
- [ ] `test_console_never_sends_atm_message`: TUI performs no delivery.
- [ ] `test_status_header_full_typed_text`: status renders typed detail.
- [ ] `test_quality_never_loaded_placeholder`: Quality has labelled empty state.
- [ ] `test_quality_split_panes`: rows show reviewer status and elapsed time.
- [ ] `test_reviewer_selection_filters_findings`: selection filters findings.
- [ ] `test_finding_lists_contributing_reviewers_filing_marked`: contributors/filer render.
- [ ] `test_missing_reviewers_metadata_placeholder`: absent metadata is labelled.
- [ ] `test_jev_api_failure_glyph_is_do_not_enter`: API failure differs from verdicts.
- [ ] `test_sanity_row_api_failure_vs_no`: API failure differs from Jev no.
- [ ] `test_diagnostics_agent_icons_coloured`: Diagnostics icons retain colour.

## Non-closure

Collection implementation, state evaluation, aggregation, and delivery remain
their producer sprints.
