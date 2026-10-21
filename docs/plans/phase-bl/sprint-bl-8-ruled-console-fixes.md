# Sprint bl-8 — ruling-gated console fixes: merge-check problems (#492) and the bd read deadline (#533)

closure_type `boundary`; target_boundary `BOUNDARY-MergeFactsAndDeadline`
(`facts.py` merge problems, `runner.py` merge-check selection,
`command.DEFAULT_TIMEOUT_S` and its bd callers). Branch
`sprint/bl-8-ruled-console-fixes`, PR → `sprint/bl-7-console-tabs` (layer 8; cut from the bl-1 head, rebased in order). Wave 2.
`external_ref`: `gh-492,gh-533`.
recommended_agent `arch-ctm`; recommended_model `sonnet`.

**Gate.** Do not start a deliverable until its decision (plan D1, D2) is
recorded verbatim in the plan. Each deliverable is independent: D2 may land
while D1 is open.

**Hard dependency.** must_follow bl-1: `runner.py` (bl-1 rewrites the
GitHub/merge gate region) and the `in_progress` refresh behaviour that D1's
options rely on.

Paths under `apps/atm-bd-console/src/atm_bd_console/`; line numbers at
v0.18.4 (86853d5).

## Owned paths

- `facts.py`, `command.py`, `runner.py` (merge-check region `:353-387`, after bl-1)
- tests: `tests/test_status_table.py`, `tests/test_command.py`,
  `tests/test_runner.py` (after bl-1)

## Facts (verified)

- "head not pushed" and "merge check failed" are display-only strings in
  the status table (`facts.py:189-194`). No ATM alert path reads them, so the
  "alert" half of #492 is already true; what remains is whether they show.
- PR-less branch pairs are still merge-checked (`runner.py:353-381`).
- Every console subprocess has one deadline, `command.DEFAULT_TIMEOUT_S = 20`
  (`command.py:11`). The close hook uses 180 s (`ATM_BD_GATE_TIMEOUT_S`). A
  first bd read on a loaded host exceeds 20 s (#533 item 4, timing data on
  #528).

## Deliverables (each per its decision)

1. **#492 (D2).** Per D2: either drop the `HEAD_NOT_PUSHED` problem, or keep
   it labelled informational ("head not pushed (informational)"); and either
   stop merge-checking PR-less pairs in `runner.py`, or keep it. If D2 keeps
   both as they are, the deliverable is the test below only, and #492 closes
   as fixed in 0.18.1.
2. **#533 item 4 (D1).** Per D1: one of (a) a longer deadline for the first
   bd read only, (b) a larger single deadline for bd reads, or (c) another
   value Rand gives. With bl-1, a long read holds only `refresh.lock`; other
   callers render the persisted result marked `in_progress`, so a longer bd
   deadline no longer blanks the console, but it delays the next refresh by
   up to that deadline.

## Authoritative acceptance criteria (deterministic)

gh-492
- [ ] `test_head_not_pushed_display_per_d2`: a merge in `HEAD_NOT_PUSHED` state produces exactly the D2 text (or none).
- [ ] `test_pr_less_pair_merge_check_per_d2`: a branch pair with no PR is (or is not) passed to the merge check, per D2.
- [ ] `test_merge_problems_never_alert`: neither problem produces an ATM action.

gh-533
- [ ] `test_bd_deadline_per_d1`: the recorded bd argv call receives the D1 deadline (first tick and later ticks as D1 says), using a fake `subprocess.run` that records `timeout`.
- [ ] `test_bd_timeout_is_typed_and_not_stamped`: a fake timeout yields a `Timeout` error carrying argv, deadline and fix, and (through bl-1's refresh) no BD success stamp.

## Authoritative validation

```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
```
