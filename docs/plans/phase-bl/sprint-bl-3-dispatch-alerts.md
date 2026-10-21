# Sprint bl-3 — alert accuracy, visible dispatch, and console-model labels

closure_type `boundary`; target_boundary `BOUNDARY-ProblemsAndDispatch`
(`model._problems`, `model._pr_rows`, `dispatch.candidates`/`dispatch.plan`, the sprint-label render data, and the CI state read by `github_source`). Branch `sprint/bl-3-dispatch-alerts`,
PR → `integrate/phase-bl`. Wave 1; no dependency. Priority P1 (erroneous or
spam ATM messages).
`external_ref`: `gh-375,gh-372,gh-491,gh-126,gh-359`.
recommended_agent `arch-ctm`; recommended_model `sonnet`.

Paths under `apps/atm-bd-console/src/atm_bd_console/`; line numbers at
v0.18.4 (86853d5). Cron and console share this code (`runner.step`), so each
fix is made once.

## Scope boundary

In: #375 (cancelled and superseded check runs; the stack top), #372 (branch
fallback; every Problem text names the bead and the fix), #491 (every ready
sanity/QA candidate that is not dispatched leaves a typed Held row; a
runner-level tick test), #126 R1 (`--exclude-label anchor` on the console's
own bd calls). Out: `runner.py` (bl-1 owns it this wave; the #126
re-read-before-assign is bl-1's), `render.py`/`app.py` (bl-6, bl-7), the
dispatcher's bead-assignee write (D4).

## Owned paths

- `model.py` (regions `_problems`, `_pr_rows`, `github_unavailable` and the
  `unknown_*` texts at `:179-196`, and `_sprint_views` / label helpers), `github_source.py`, `atm_bd.py`,
  `dispatch.py`, `beads_source.py`
- tests: `tests/test_model.py`, `tests/test_github_source.py`,
  `tests/test_atm_bd.py`, `tests/test_dispatch.py`, `tests/test_beads_source.py`,
  new `tests/test_dispatch_tick.py`

No other sprint owns `model.py`; dashboard rendering consumes its produced label data without editing this file.

## Relations

`parallel_safe`: bl-1, bl-2, bl-4 (disjoint paths and no consumed artifact).
`must_follow`: none. bl-6 consumes the rendered model data but does not own or
edit `model.py`; its renderer remains a separate boundary.

## Public contract

```python
# github_source.py
FAILURES: frozenset[str]            # CANCELLED removed (today :19)
def latest_per_name(runs: Iterable[CheckRun]) -> tuple[CheckRun, ...]: ...
    # one run per check name: highest id, ties by started_at; used by the REST (~:260) and GraphQL (:160-163) paths
# atm_bd.py
def step_branch(bead: Bead, containers: Mapping[str, Bead]) -> str | None: ...
    # bead.metadata.branch, else metadata.sprint_bead container's metadata.branch, else None
# dispatch.py: every ready sanity/QA candidate yields exactly one of: an Action, or a Held row
HeldReason = Literal["render_error", "sprint_not_found", "target_not_found",
                     "worktree_missing", "branch_missing", "inputs_unavailable"]
```

## Authoritative deliverables

1. **#375 CI.** Reduce check runs to the latest run per name before
   computing CI state, in both the REST and GraphQL paths; drop `CANCELLED`
   from `FAILURES`, so a cancelled run counts as failed only if it is the
   latest of its name and then reads as "cancelled", not "failed".
   `ci_state_of` (`:109`) uses the reduced set.
2. **#375 top.** `_pr_rows` (`model.py:424-426`) computes "top" over every
   open PR in the base chain (any open PR whose base is another chain PR),
   not only bead-owned PRs, so a layer linked above is the top.
3. **#372 branch.** `atm_bd.py:337` resolves a step's branch through
   `step_branch`: its own `metadata.branch`, else the container named by
   `metadata.sprint_bead`.
4. **#372 texts.** Every `Problem` text in `model._problems` names the bead
   id, what is missing, and the fix (for a missing branch: "set
   metadata.branch on bead X (or on its sprint container Y)"). The
   `github_unavailable` and `unknown_*_state` texts (`model.py:179-196`) gain
   the fix text.
5. **#491 typed Held rows.** In `dispatch.py`, the spec-render error
   (`:157-159`), the sprint/target NotFound cases (`:140-145`) and the silent
   `continue` on a missing worktree or branch (`:146-147`) each produce a
   Held row with a `HeldReason` and the typed error text, delivered on the
   same notification path as other Held rows. No ready candidate is dropped
   silently.
6. **#491 tick test.** New `tests/test_dispatch_tick.py`: drive
   `runner.step` with fake bd/atm/git/gh sources from a sanity-PASS bead at a
   valid PR head, lead idle, QA bead with an assignee (the #491 root cause),
   and assert the tick's actions include the QA `assign`. #359's sprint-label data also lives here, so no later sprint shares `model.py`. A second case with
   the bd read failing asserts a visible "dispatch refused" Held row
   (`inputs_unavailable`) and no assign.
7. **#126 R1.** `beads_source.ready()` and `open_rows()` pass
   `--exclude-label anchor` (as `queues.py:150` does).
8. **#359 sprint labels.** `_sprint_views` builds each node label as exactly
   `<state icon> <sprint id>` plus ` <agent icon>` per active agent, single
   spaces, no role word, no agent name, and no title lines. Pad each label to
   the widest so mmdflux boxes have equal width. This is owned here because
   this sprint owns all of `model.py`.

## Authoritative acceptance criteria (per issue, deterministic)

gh-375
- [ ] `test_cancelled_superseded_run_not_failed`: the #1892 fixture (run id N "Format check" cancelled, run id N+1 same name success) gives CI state success, REST and GraphQL.
- [ ] `test_latest_cancelled_reads_cancelled`: a single cancelled run is not reported as "failed".
- [ ] `test_top_includes_non_bead_layer`: PRs A (bead-owned) and B (base = A's branch, no bead) → top is B.

gh-372
- [ ] `test_group_bead_branch_from_container`: a group-sanity bead with no branch under a container with `branch` raises no problem.
- [ ] `test_missing_branch_text_names_bead_and_key`: a bead with neither yields text containing the bead id and `metadata.branch`.
- [ ] `test_every_problem_text_names_bead`: for each problem kind built in `_problems`, the text contains the bead id.

gh-491
- [ ] `test_qa_bead_with_assignee_dispatched` (in `test_dispatch_tick.py`): the tick emits the QA assign.
- [ ] `test_each_skip_is_typed_held`: one fixture per `HeldReason` gives exactly one Held row with that reason.
- [ ] `test_bd_failure_is_refused_row` (in `test_dispatch_tick.py`).

gh-126 (R1)
- [ ] `test_ready_and_open_exclude_anchor`: the recorded bd argv for `ready` and `open_rows` contains `--exclude-label anchor` and no `--all`.

## Authoritative validation

```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
```

No test runs bd, dolt, atm or gh: every source is a fake that returns typed
`Result`s.

## Unverified

- That poured group beads carry no `metadata.branch` (the #372 cause) was not
  confirmed against live bead data; the fallback is correct either way.
- The #491 2h26m window's root cause beyond the assignee filter (fixed in
  v0.18.1) was not re-derived from tick logs.
