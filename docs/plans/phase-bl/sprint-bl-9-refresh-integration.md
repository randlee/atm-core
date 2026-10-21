# Sprint bl-9 — refresh integration: cron and console end to end, every control, live phase run

closure_type `integration`; target_boundary `BOUNDARY-Refresh` end to end
(`atm-bd cron` and `atm-bd console` on one phase state dir). Branch
`sprint/bl-9-refresh-integration`, PR → `sprint/bl-7-console-tabs`; bl-8 is decision-gated and is not an integration prerequisite. Wave 3.
Priority P1. `external_ref`: `gh-126`.
recommended_agent `arch-ctm`; recommended_model `opus`.

**Hard dependencies.**
- must_follow bl-7: the console tick on `refresh.refresh` and the tab
  renderers (`app.py`).
- must_follow bl-6: `render.dashboard` / status bar with `content_at`.
- (bl-1 is below both.)

Anchors: `design.md` §1. Paths under `apps/atm-bd-console/`.

## Relations

`must_follow` bl-6 for the carried-content render contract and bl-7 for the
real console tick/tab contract. `parallel_safe`: bl-2, bl-3, bl-4, bl-5, and
the D1/D2-blocked bl-8. This sprint owns only its integration tests and live
proof, including all feature-level refresh cases (a), (b), and (c), never the producer paths.

## Scope boundary

In: tests that drive the real `cli.main(["cron", ...])` and the real
`app.run` (Textual pilot) against one `tmp_path` state dir with fakes only at
the subprocess boundary (`command.run`), and one live run on a real phase.
Out: new product behaviour. A defect found here is fixed in this sprint only
if it is inside `tests/`; a product defect becomes a fix bead on the owning
sprint's file set.

## Owned paths

- new `tests/test_refresh_uniform.py`
- new `tests/fixtures/refresh/` (scripted bd/atm/gh/git replies)

No `src/` file.

## Authoritative deliverables

1. **Scripted end to end.** A fake `command.run` replays scripted replies
   per argv and records every call. Scenarios, each with an injected clock:
   - cron at t=0 succeeds; console at t=10 makes zero bd/atm/gh calls and
     shows cron's snapshot, sanity runs and QA runs;
   - console at t=0, cron at t=10: cron makes zero calls;
   - bd timeout at t=0 in cron: console at t=1 re-reads bd;
   - GitHub 502 at t=0: the next call re-reads GitHub;
   - a never-read state: every control shows its labelled placeholder;
   - a failure after a good read: every control shows the last good
     content, marked stale with `content_at` age and the typed reason.
2. **Per-control checklist in the test.** One parametrized test over the
   console controls still in scope after v0.18.4 (dashboard header, agents,
   errors, Held, status header, status table, sprint list, PRs tab, status
   bar, Sanity tab, Quality tab, findings pane, Diagnostics, DAG pane), each
   in the states never-read / fresh / stale-after-failure, asserting: no
   blank and no Python repr; the placeholder or stale marker text; the typed
   error text present on screen and in the console log.
3. **Live proof.** The lead records a real phase observation without a load
   generator. The PR body reports only persisted `refresh.json` data, cron
   stdout, and console logs (`by`, `ran`, `skipped_reason`); no evidence file
   is committed.

## Authoritative acceptance criteria

gh-126 (a) identical
- [ ] `test_cron_then_console_same_screen`: the console's dashboard text after cron's refresh equals the text the console renders after its own refresh of the same scripted replies.

(b) no doubled load
- [ ] `test_call_count_two_entry_points`: cron (t=0) + console (t=10) + console (t=70) + cron (t=75): bd and atm argv each called exactly twice; gh called once (600 s window).

(c) failure never suppresses
- [ ] `test_failed_bd_then_other_entry_rereads`.
- [ ] `test_failed_github_rereads_next_call`.

Per control
- [ ] `test_every_control_three_states` (parametrized, deliverable 2).

Live
- [ ] The PR body carries the live-run output of deliverable 3, showing at least one console refresh skipped as `fresh` after a cron refresh and no skip after a failed read (if no read failed during the run, say so).

## Authoritative validation

```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
```

No sleeps or threads in tests; the live run is the only real-time part and
is not a test.

## No-stamp integration case

`test_no_success_stamp_for_failed_source` is owned here: a controlled failed
source read writes no success stamp, then the next entry point re-reads it.
This is distinct from the producer unit tests and is one of the feature-level
(a)/(b)/(c) end-to-end cases.
