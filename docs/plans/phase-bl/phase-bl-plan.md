# Phase bl — atm-bd-orchestration 0.18.5

## Goal

Release atm-bd-orchestration 0.18.5 closing the fourteen issues labelled
`0.18.5` in randlee/atm-bd-orchestration: #379 #164 #365 #497 #375 #492 #372
#126 #533 #498 #491 #359 #360 #347, each to its narrowed scope (2026-10-21
review) and the rulings below. The core track is one shared console refresh
for `atm-bd cron` and `atm-bd console` (`design.md`).

Code repo: randlee/atm-bd-orchestration. Integration branch
`integrate/phase-bl`, cut from `main` (that repo has no `develop`). Base
code: v0.18.4 (86853d5); every line reference in the sprint docs is at that
commit. atm-core holds this plan and the beads (atm-core beads database,
`atm-*` ids). Lead: team-lead.

## Rulings and rules recorded for this phase (verbatim)

Rand, 2026-10-21, refresh (the anchor of `design.md`):

> "my expectation for 'atm-bd cron' and 'atm-bd console' is that both are run on a specific phase. They both 'schedule' a info refresh which a) is exactly the same regardless of which system instigated it. b) is checked for duplication so running both does not DOUBLE the query load on bd/dolt, atm, github c) A failed run which did not produce successful data does NOT cause a run to be skipped."

Rand, 2026-10-20, per console control: "if you don't have valid data, don't
force an error."

Rand, 2026-10-21, relayed by the coordinator (paraphrase, not a quote; the
full relay is in `design.md` §1): the refresh must update every console
control correctly and the cron-produced result must be what the console
shows; per-source success stamps shared by cron and console; a failed or
partial run never counts as success and never suppresses the next run.

Rand, 2026-10-21, on #365 and #497: quoted verbatim in
`sprint-bl-2-jev-client.md`. They are dev-sanity requirements of
atm-bd-orchestration (`docs/requirements/dev-sanity.md`, PR in flight), not
phase-bl anchors; the sprint docs cite their requirement numbers once that
PR lands.

Issue classification (Rand, 2026-10-21):
- P1 = erroneous or spam ATM messages, Jev errors or misuse, or the TUI
  showing improper information.
- TUI colour and layout issues are diagnosis aids (P3 minimum).
- Jev is a critical failure point in the skill.

Execution rules for this phase:
- Sprint agents do not read GitHub issues. Each sprint bead carries its
  sprint doc's closing criteria and `--external-ref` (the comma-joined
  `gh-<n>` list in the doc header).
- Each sprint doc gives 2-5 deterministic closing criteria per issue.
- No test sleeps, uses threads, or runs bd, dolt, atm or gh; clocks, sleeps
  and subprocesses are injected.
- `CHANGELOG.md` and the version bump are written by the 0.18.5 release
  step, not by sprints.
- No sprint adds a plugin file, so `plugins/atm-bd-orchestration/manifest.yaml`
  is unchanged; a sprint that finds it must add one stacks the manifest
  regeneration in its own layer and says so in its PR.

## Tracks

| Track | Sprints | Issues |
| --- | --- | --- |
| Refresh (core) | bl-1 → bl-6, bl-7 → bl-9 | #126 (refresh), audit remainder |
| Jev | bl-2 → bl-5 | #365, #497, #379, #164, #498, #126 item 4 |
| Alert accuracy | bl-3, bl-8 | #375, #372, #491, #126 R1, #492, #533 |
| TUI diagnosis | bl-4 → bl-7, bl-6 | #347, #359, #360 |

## Sprints

Layers are one append-only stack; each PR targets the layer below. Parallel
sprints are cut from their dependency's head and rebased into the stack in
layer order.

| Layer | Sprint | Wave | Closure | Issues (`external_ref`) | Owned paths (summary; full list in the doc) | Model |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | bl-1 refresh-core | 1 | contract | gh-126 | console `refresh.py` (new), `state.py`, `poll_health.py`, `runner.py`, `step_result.py`, `cli.py`, `types.py`, `codec.py`, `closed_cache.py`; their tests; plugin `SKILL.md` cron paragraph | opus |
| 2 | bl-2 jev-client | 1 | contract | gh-365, gh-497, gh-379, gh-164 | `jev_client.py`, `wrap_jev_receipt.py`, `sanity-split`, `agents/sc-sanity-jev.md`; tests jev_client, wrap, sanity_split, agent_prompts | opus |
| 3 | bl-3 dispatch-alerts | 1 | boundary | gh-375, gh-372, gh-491, gh-126 | console `model.py` (problems/PR rows/texts), `github_source.py`, `atm_bd.py`, `dispatch.py`, `beads_source.py`; tests incl. new `test_dispatch_tick.py` | sonnet |
| 4 | bl-4 qa-reviewer-data | 1 | contract | gh-347 | `roles/quality-mgr.md`, `qa-template.xml.j2`, `finding-bead.json.j2`, their examples, `test_qa_report_contract.py` | sonnet |
| 5 | bl-5 sanity-flow | 2 | boundary | gh-365, gh-497, gh-498, gh-126 | `agents/dev-sanity.md`, `dev-sanity-template.xml.j2`, `sanity-run-record.json.j2`, examples, `sanity-merge`, `sanity-run-history`, `assignment-gates.py`, `sanity-stack-link`, `atm-task-close.py`; their tests incl. `test_templates.py`, `test_handoff_contracts.py`, `test_task_close.py`, `test_agent_prompts.py` | opus |
| 6 | bl-6 dashboard-render | 2 | boundary | gh-126, gh-359, gh-360 | console `render.py`, `model.py` (label region), `status.py`, `cli.py` (`show`), `README.md`; tests | sonnet |
| 7 | bl-7 console-tabs | 2 | boundary | gh-126, gh-347, gh-365, gh-360 | console `app.py`, `icons.py`, `runs.py`; tests app, tabs, icons, runs | opus |
| 8 | bl-8 ruled-console-fixes | 2 | boundary | gh-492, gh-533 | console `facts.py`, `command.py`, `runner.py` (merge region); tests | sonnet |
| 9 | bl-9 refresh-integration | 3 | integration | gh-126 | new `tests/test_refresh_uniform.py`, `tests/fixtures/refresh/`; live run | opus |

`owned_paths` do not intersect within a wave. Files shared across waves are
sequenced by the hard dependencies below.

## Wave table

| Wave | Sprints | Hard dependency (consumed API or artifact) |
| --- | --- | --- |
| 1 | bl-1, bl-2, bl-3, bl-4 | none |
| 2 | bl-5 | ← bl-2: `jev_client.py --requests`, its printed result (`results[]`, `error.kind`, `retries`, `elapsed_s`), `JevErrorKind`; and `test_agent_prompts.py` after bl-2's edit |
| 2 | bl-6 | ← bl-1: `Snapshot.content_at`, `PollHealth.restore`/`health.json`, bl-1's `cli.py`; ← bl-3: `model.py` after `_problems`/`_pr_rows` edits, `test_model.py` |
| 2 | bl-7 | ← bl-1: `refresh.refresh(by="console")`, `Refreshed`, `acquire_console_lock`, `runs.json` `Held` values; ← bl-4: `metadata.reviewers` shapes (QA bead and finding bead) |
| 2 | bl-8 | ← bl-1: `runner.py` after the GitHub/merge gate rewrite, refresh `in_progress` behaviour; gated on D1/D2 |
| 3 | bl-9 | ← bl-6: dashboard/status bar `content_at` rendering; ← bl-7: console tick on `refresh.refresh`, tab renderers |

Critical path: 3 waves (bl-1 → bl-7 → bl-9). Width: 4 (waves 1 and 2).
Sprint count: 9.

DAG: `docs/plans/phase-bl.jsonl`.

## Open decisions (Rand's)

Work that does not depend on these starts now; each names what waits.

| # | Issue | Question | What waits |
| --- | --- | --- | --- |
| D1 | #533 item 4 | The console bd read deadline is 20 s (`command.py:11`) and fails on the first tick on a loaded host. A 180 s deadline (the close hook's `ATM_BD_GATE_TIMEOUT_S`) would stall the 60 s tick. Options: a longer first-tick-only deadline, a larger single bd deadline (with bl-1 a long read only delays the next refresh; other callers show the persisted result marked in progress), or another value. Rand's decision. | bl-8 deliverable 2 |
| D2 | #492 | Should "head not pushed" (display-only today, `facts.py:189-190`; no ATM alert exists) be dropped or shown as informational? Should PR-less branch pairs still be merge-checked (`runner.py:353-381`)? | bl-8 deliverable 1 |
| D3 | #498 item 3 | Is the notice to the lead on `rebase_required` (`sanity-stack-link:250`, today) enough, or must the bead also be reopened? | bl-5 deliverable 6 (the test of today's behaviour is not gated) |
| D4 | #126 remainder | Not planned, pending a ruling on each: 1 candidate-scoped GitHub read before dispatch (adds GitHub load, against requirement (b)); 3/5a QA gate on "top not red" (CLAUDE.md: "CI is a merge gate only — never a dispatch gate"); 5b reviewers from bead metadata; 6 dispatcher `gh stack link` (overtaken by sanity-stack-link on PASS); 7b launchd/herdr supervision; 7c escalation recipients; R4 `pulls?head=` lookup (adds GitHub calls); 2b's bead-assignee write before assign (interacts with the ANC-057 assignee rules; the task re-read of 2b is in bl-1). R5 (pouring important and minor findings) is a standing ruling with no work found. | nothing in this plan |
| D5 | #365 / #497 | When Jev returns no evaluation (any `results[i].success == false`), which reply decides the deliverable: the LLM reply mechanically (as #497's text says), or CANNOT_RUN (as #365's original "never silently substitute" says)? The agent never decides either way. | bl-5 deliverable 3's selection change (recording and icon are not gated) |

Not planned: #497's earlier "split inconclusive" error. Under the 2026-10-21
ruling it can only be computed by reading each reply's `choice`, which is
interpreting verdicts; it is left out unless Rand rules for it.

## Governed interfaces

- `jev_client.py` stdout (bl-2): a breaking change from
  `{success,data,error,elapsed_s}` with `data.parts` to the request-array
  result; its only consumers are `wrap_jev_receipt.py`, `sanity-merge` and
  `sanity-run-history`, all changed in bl-2/bl-5 of this stack.
- Sanity-log row (`sanity-run-record.json.j2` 4.0.0 → 5.0.0, bl-5): adds
  `jev_requests`, `jev_results`, `jev_retries`; read by the console
  (`runs.py`, which reads `kind` and keeps working on older rows).
- Console state dir (bl-1): new `refresh.json`, `health.json`, `runs.json`,
  `refresh.lock`, `console.lock`; `snapshot.json` gains `content_at`. Read
  only by the console package itself.
- Bead metadata (bl-4): additive `metadata.reviewers` on QA and finding
  beads.

## Exit criteria

- Every sprint's acceptance criteria pass at its merged head, and the
  console and plugin validation commands pass on `integrate/phase-bl`.
- bl-9's live run on a real phase is in its PR body: a console refresh
  skipped as `fresh` after a cron refresh, and no skip after a failed read.
- Each of the fourteen issues is closed with its closing sprint(s) and
  merged head named, or left open naming what remains (#498's live timing;
  #126's D4 items; #492/#533 if D1/D2 are not ruled).
- The 0.18.5 release step writes `CHANGELOG.md`, bumps the versions, tags
  `v0.18.5` on the installed commit, and installs it into atm-core.
