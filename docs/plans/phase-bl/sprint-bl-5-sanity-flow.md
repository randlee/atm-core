# Sprint bl-5 — sanity flow: LLM-written Jev requests, verbatim Jev results in the sanity log, mechanical steps

closure_type `boundary`; target_boundary `BOUNDARY-SanityRecord` (the
sanity-log row written by `sanity-run-history`, and the dev-sanity steps that
produce it). Branch `sprint/bl-5-sanity-flow`, PR → `sprint/bl-4-qa-reviewer-data`
(layer 5; cut from the bl-2 head, rebased in order). Wave 2. Priority P1.
`external_ref`: `gh-365,gh-497,gh-498,gh-126`.
recommended_agent `arch-ctm`; recommended_model `opus`.

**Hard dependency.** must_follow bl-2. It consumes from bl-2:
`jev_client.py --requests <file>`, the printed result (`results[]` with
`success`, `data`, `receipt`, `error.kind`, `retries`, `elapsed_s`), and
`JevErrorKind`. It also edits `test_agent_prompts.py` after bl-2.

Paths under `plugins/atm-bd-orchestration/` unless stated. Line numbers at
v0.18.4 (86853d5).

## Requirement source (verbatim)

The #365 and #497 rulings are dev-sanity requirements of
atm-bd-orchestration (`docs/requirements/dev-sanity.md`, PR in flight), not
phase-bl anchors; cite their requirement numbers once that PR lands.

Rand, 2026-10-21, on #365:

> "on #365, prompt rule should not be at play here. the jev call should be returned. I do NOT want an agent 'intrepeting' what the jev python call returns. the return must be json and included in the final result/data written to sanity log. the elapsed time for the jev call is calculated in jev-py, jev-py includes retry for known API 'busy' error (it does not stupidly retry on other errors like out-of-tokens, context-too-large). jev should return an enum with all known errors which is exhaustively checked in match. jev should return # of retries in the case of api-busy. jev icon/error in tui dashboard should show 'do-not-enter' icon if jev did not return an evaluation so it is not confused w/ jev returning negative response (from a properly functioning model call)"

Rand, 2026-10-21, on #497:

> "then it is a poor design and will be removed. context packing LLM must split and send array of requests to jev."

Rand, #498 target flow (issue body): "the sanity agent (1) passes the bead
*content* to the Jev and LLM subagents in parallel (subagents never look up
the bead), (2) reconciles their answers, (3) reports and closes, (4) hands off
to sanity-stack. Any other step in the sanity path is a defect; changes to
this path must be mechanical (scripts)."

#126 item 4 (issue body): "Bootstrap belongs to the sanity agent. It moves
into the sanity template's first step, so the sanity check does not depend on
the lead having bootstrapped the worktree (observed failure: sanity lint exit
127, missing `.bootstrap-venv`)."

## Scope boundary

In: the dev-sanity side of #365 and #497 (the LLM writes the request array;
the array and the client's JSON are copied unchanged into the sanity-log
row), #498's remaining items, #126 item 4. Out: the client (bl-2), the
console icon (bl-7). Which reply decides a deliverable when Jev returned no
evaluation is decision D5; this sprint does not change selection beyond
removing the agent's interpretation of Jev output, and implements D5 only
once it is recorded. The rebase_required reopen is decision D3.

No new plugin file (`manifest.yaml` untouched).

## Owned paths

- `agents/dev-sanity.md`
- `skills/atm-bd-orchestration/templates/dev-sanity-template.xml.j2`,
  `templates/sanity-run-record.json.j2`
- `skills/atm-bd-orchestration/examples/dev-sanity-template-vars.json`,
  `examples/sanity-run-record-vars.json`
- `skills/atm-bd-orchestration/scripts/sanity-merge`, `scripts/sanity-run-history`,
  `scripts/assignment-gates.py`, `scripts/sanity-stack-link`,
  `scripts/atm-task-close.py`
- tests: `scripts/tests/test_templates.py`, `test_handoff_contracts.py`,
  `test_task_close.py`, `test_agent_prompts.py` (after bl-2),
  `test_sanity_merge.py`, `test_sanity_run_history.py`,
  `test_assignment_gates.py`, `test_sanity_stack.py`

`test_templates.py` and `test_handoff_contracts.py` also hold QA-template
tests; bl-4 does not edit those two files (its tests go in
`test_qa_report_contract.py`).

## Public contract

Sanity-log row (template `sanity-run-record.json.j2` 5.0.0) gains two
required fields on a `sanity-jev` row, empty arrays on `sanity-llm`:

```json
"jev_requests": [ <the request array the LLM wrote, verbatim> ],
"jev_results":  { <jev_client.py stdout, verbatim: model, results[], retries, elapsed_s> }
```

- `jev_elapsed_s` = `jev_results.elapsed_s`; `jev_retries` = `jev_results.retries`
  (new field). Neither is computed by the agent.
- `kind` = `"API_FAILURE"` iff some `jev_results.results[i].success` is false,
  decided by an exhaustive `match` on `JevErrorKind` in `sanity-run-history`
  (no string prefix test). The console (bl-7) shows the do-not-enter icon
  for this `kind`, distinct from a Jev `no`.
- `jev_receipts` is replaced by the receipts inside `jev_results`.

## Authoritative deliverables

1. **LLM writes the request array (#497).** `dev-sanity.md` and the
   template's step b: the coordinator, which holds the full context, writes
   for each deliverable a JSON array of one or more Jev requests (shape:
   `validate_request`), each under `MAX_REQUEST_BYTES`, to a file outside the
   worktree, and hands that file to the `sc-sanity-jev` child. Remove every
   instruction that relies on the client to build or split requests, or that
   tells the agent to trim context.
2. **Verbatim record (#365).** `sanity-run-history` reads the request file and
   the client stdout file by path (new options `--jev-requests <file>` and
   `--jev-results <file>`) and writes them into the row unchanged. The agent
   passes paths, never values. `kind`, `jev_elapsed_s` and `jev_retries` are
   derived there. Remove the `code.startswith("SANITY.JEV_")` matcher
   (`sanity-run-history:53-56`).
3. **No agent interpretation (#365).** Remove "selection takes the LLM reply"
   from `sanity-merge` `screen_jev_results` (`:290-305`) and the matching text
   in `dev-sanity.md`; a Jev result with an error is passed through as
   recorded and never rewritten by the agent. The D5 outcome is applied here
   when recorded.
4. **NOT_READY is mechanical (#498 item 7).** `assignment-gates.py sanity`
   prints, on NOT_READY, the bead's open blockers and its status (what
   template step a now asks the agent to get with `bd show` and `bd blocked`,
   line 53), and the template step a drops those two commands.
5. **Epochs are derived (#498 item 4).** `sanity-merge` (`--started-at`,
   `--completed-at`, `:494-558`) and `sanity-run-history` take the start and
   completion epochs from the reviewer reply files' recorded times (the
   client's `elapsed_s` for Jev; the files' write order for the LLM), so step
   b no longer asks the agent to record epochs.
6. **rebase_required (#498 item 3).** Confirm in a test that
   `sanity-stack-link` (`:250`) notifies and does not reopen, and document
   that in `dev-sanity.md`. Reopening is implemented only if D3 says so.
7. **not_reproducible (#498 item 6).** `atm-task-close.py` (`:109-111`,
   `:248`) today checks only that the cited `file:line` exists. Make it also
   check that the cited line at the pinned commit contains the text the claim
   quotes (`claim_text` var), and refuse with the existing `VARS.INVALID`
   shape otherwise.
8. **Bootstrap first (#126 item 4).** Template step a runs the worktree
   bootstrap (the repo's `just bootstrap`, or the command in
   `close_context.bootstrap_command` when set) before `assignment-gates.py`,
   and fails the task as CANNOT_RUN with the command's stderr when it fails.

## Authoritative acceptance criteria (per issue, deterministic)

gh-365
- [ ] `test_row_copies_jev_json_verbatim`: given a request file and a client stdout file, the appended row's `jev_requests` and `jev_results` are `==` to the files' JSON.
- [ ] `test_kind_from_error_enum_exhaustive`: one fixture per `JevErrorKind` member gives `kind == "API_FAILURE"`; all-success gives `"verdict"`; a member missing from the match fails the test.
- [ ] `test_retries_and_elapsed_from_client`: row `jev_retries == 2`, `jev_elapsed_s == 4.0` from a client file carrying those values.
- [ ] `test_no_llm_selection_text`: `sanity-merge` and `dev-sanity.md` contain neither "selection takes the LLM reply" nor "selects the LLM reply".

gh-497
- [ ] `test_template_step_b_has_request_array`: the rendered step b names the request-array file and `jev_client.py --requests`; it contains no "trim" / "narrow context" instruction.
- [ ] `test_row_records_request_array`: a two-request array is recorded with both elements in order.

gh-498
- [ ] `test_not_ready_prints_blockers`: a fixture bead blocked by `x-1` makes `assignment-gates.py sanity` print `NOT_READY` with `x-1` and the status; rendered step a contains no `bd show` or `bd blocked`.
- [ ] `test_merge_without_epoch_flags`: `sanity-merge` runs without `--started-at/--completed-at` and records the derived values.
- [ ] `test_rebase_required_notifies_without_reopen` (or `..._reopens` once D3 is recorded).
- [ ] `test_not_reproducible_claim_text_checked`: a cited line whose text differs from `claim_text` is refused with `VARS.INVALID`.

gh-126 (item 4)
- [ ] `test_step_a_bootstraps_first`: rendered step a runs the bootstrap command before `assignment-gates.py`.
- [ ] `test_bootstrap_failure_is_cannot_run`: the template text routes a failed bootstrap to CANNOT_RUN with stderr.

The #498 acceptance timing (PASS closes within lint + review + ~30 s) is not
a test here: it is measured from `atm task events` on the first real phase
run with 0.18.5 installed, and #498 stays open until then. The bootstrap
command name (`just bootstrap`) is not verified against every consuming repo;
the dev confirms it for atm-core and atm-bd-orchestration.

## Authoritative validation

```bash
cd plugins/atm-bd-orchestration && python3 tests/gen_manifest.py --check && python3 -m pytest -q -rs
```

## Non-closure

#365 closes when bl-2, this sprint and bl-7 (icon) are merged. #498 stays
open until the live timing is measured.
