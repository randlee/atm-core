# Sprint bl-7 — console tabs on the shared refresh: persisted values, Quality reviewer pane, Jev do-not-enter icon

closure_type `boundary`; target_boundary `BOUNDARY-ConsoleApp` (`app.py`:
the tick, the Status, Sanity, Quality and Diagnostics tabs). Branch
`sprint/bl-7-console-tabs`, PR → `sprint/bl-6-dashboard-render` (layer 7; cut
from the bl-4 head after bl-1, rebased in order). Wave 2.
Priority: the refresh adoption and the do-not-enter icon are P1 (TUI showing
improper information; Jev); the Quality reviewer pane and colour are P3.
`external_ref`: `gh-126,gh-347,gh-365,gh-360`.
recommended_agent `arch-ctm`; recommended_model `opus`.

**Hard dependencies.**
- must_follow bl-1: consumes `refresh.refresh(config, by="console", force=..., clock=...)`,
  `Refreshed` (`result`, `ran`, `skipped_reason`), `state.acquire_console_lock`,
  `runs.json` `Held` values, and `PollHealth` applied inside the refresh.
- must_follow bl-4: consumes the QA bead `metadata.reviewers[]` and finding
  bead `metadata.reviewers` shapes in `sprint-bl-4-qa-reviewer-data.md`
  (Public contract).

Paths under `apps/atm-bd-console/src/atm_bd_console/`; line numbers at
v0.18.4 (86853d5).

## Governing text (verbatim)

Rand, 2026-10-21 (requirement source for the icon; a dev-sanity requirement
in atm-bd-orchestration `docs/requirements/dev-sanity.md`, PR in flight):

> "... jev icon/error in tui dashboard should show 'do-not-enter' icon if jev did not return an evaluation so it is not confused w/ jev returning negative response (from a properly functioning model call)"

Rand on #347: "on qa page, we should probably create 2 horizontal panels
(top/bottom).  top would be ~60% and would have most recent qa run on top.
when selecting a row of a qa run, the lower pane would have a vertical split
each qa subagent w/ status (pass/fail Nerd icon), elapsed time of subagent,
if subagent was selected,  finding text for all findings would show up in
lower right panel which rendered it nicely."

Rand, 2026-10-20: "if you don't have valid data, don't force an error."
Refresh anchors: `design.md` §1.

The comment at `icons.py:78` ("the lead ruled to retain U+F0026 #D29922")
is superseded for the JEV column by the 2026-10-21 ruling.

## Scope boundary

In: the console tick moves onto `refresh.refresh`; the app stops applying
`PollHealth` itself and stops keeping `State` in memory; Sanity and open-QA
values come from the refresh result (so a cron refresh shows up in the
console); console-audit rows still open at v0.18.4 in `app.py`: row 7
(Status header prints only kind and fix for `status_error`, `app.py:765`) and
row 14 (no explicit "never loaded" placeholder row in the Quality list);
#347 Quality pane; #365 icon; #360 Diagnostics path (`app.py:816`). Out:
dashboard and DAG (bl-6), the producer (bl-4).

## Owned paths

- `app.py`, `icons.py`, `runs.py`
- tests: `tests/test_app.py`, `tests/test_tabs.py`, `tests/test_icons.py`,
  `tests/test_runs.py`

`test_tabs.py:94,101` and `test_icons.py:27,31` assert the old JEV glyph and
are updated here.

## Authoritative deliverables

1. **Console on the shared refresh (#126).** `app.run` (`:825-839`) takes
   `state.acquire_console_lock` (second console still exits 1); the tick
   calls `refresh.refresh(by="console")` and renders `Refreshed.result`; the
   `r` key and PR-tab activation pass `force` (`{GITHUB}` for the PR tab,
   every source for `r`). Remove the app's own `health.apply` (`:601`) and the
   in-memory `State` (`:572,834`). A `skipped_reason="in_progress"` result
   renders normally with a "refresh in progress (<by>)" note.
2. **Sanity/Quality from the refresh.** `sanity_held` and `open_quality_held`
   (`:398-400`) are replaced by the `runs.json` values in the refresh result.
   Closed QA runs stay read on tab activation, through the same `Held`.
3. **Status header (row 7).** Print the full typed text (`error_text`) and
   age for `status_error`, not only kind and fix.
4. **Quality never-loaded row (row 14).** An empty, never-loaded Quality
   list shows a labelled placeholder row (`Open QA runs: not loaded yet` or
   the typed error), never a blank list.
5. **Quality reviewer pane (#347).** The Quality tab is two horizontal panes
   (top about 60%: QA runs newest first). Selecting a run fills the bottom
   pane, split vertically: left, one row per `metadata.reviewers[]` entry
   (reviewer, PASS/FAIL icon, `elapsed_s`, the API_FAILURE icon when `kind`
   is API_FAILURE); right, the findings Markdown (existing
   `qa-findings.md.j2` render path). Selecting a reviewer filters the
   findings to its `findings` ids; each finding lists all contributing
   reviewers with the filing reviewer marked; selecting the run shows each
   finding once. A QA bead without `metadata.reviewers` shows
   `reviewers: not recorded` (beads poured before 0.18.5).
6. **Do-not-enter icon (#365).** `JEV_VERDICT_ICONS["API_FAILURE"]`
   (`icons.py:78-81`) becomes a Nerd Font do-not-enter glyph, distinct from
   every JEV verdict glyph (candidate `nf-md-minus_circle` U+F0376; the dev
   verifies the codepoint against the Nerd Fonts cheat sheet and cites it in
   the PR). The Sanity tab's JEV cell (`_verdict`, `sanity_text`, `:153,196`)
   shows it whenever the row's `kind` is `API_FAILURE`, with the typed
   error in the row detail; a Jev `no` keeps the FAIL glyph.
7. **Diagnostics colour (#360).** The Diagnostics agent details (`:816`)
   keep Rich `Text` instead of `.plain`.

## Authoritative acceptance criteria (deterministic; Textual `run_test` pilot, fake refresh)

gh-126
- [ ] `test_console_tick_uses_shared_refresh`: the app calls the injected `refresh` with `by="console"` and renders its `result` unchanged.
- [ ] `test_console_shows_cron_result`: the fake refresh returns a snapshot generated by a `by="cron"` run → the dashboard shows it, and the app performs no read of its own.
- [ ] `test_r_forces_all_sources_pr_tab_forces_github`.
- [ ] `test_status_header_full_typed_text`: a `Timeout` status error renders argv, deadline and fix text.
- [ ] `test_quality_never_loaded_placeholder`.

gh-347
- [ ] `test_quality_split_panes`: selecting a run shows one left-pane row per `metadata.reviewers` entry with its icon and elapsed.
- [ ] `test_reviewer_selection_filters_findings`: three reviewers over two findings → selecting reviewer 2 shows only its finding ids.
- [ ] `test_finding_lists_contributing_reviewers_filing_marked`.
- [ ] `test_missing_reviewers_metadata_placeholder`.

gh-365
- [ ] `test_jev_api_failure_glyph_is_do_not_enter`: `JEV_VERDICT_ICONS["API_FAILURE"]` is the chosen glyph and differs from every JEV verdict glyph.
- [ ] `test_sanity_row_api_failure_vs_no`: a row with `kind: API_FAILURE` renders the do-not-enter glyph; a row with Jev verdict FAIL renders the FAIL glyph.

gh-360
- [ ] `test_diagnostics_agent_icons_coloured`: each icon span carries its anchored hex colour.

## Authoritative validation

```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
```

No sleeps: the Textual pilot drives key presses; the refresh and the clock
are injected fakes.
