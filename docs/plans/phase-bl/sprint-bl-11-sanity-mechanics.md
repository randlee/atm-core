# Sprint bl-11 — close and stack sanity mechanics

closure_type `boundary`; target_boundary `BOUNDARY-SanityMechanics`. Branch
`sprint/bl-11-sanity-mechanics`, PR → `sprint/bl-10-problem-sources`. Wave 1;
no dependency. `external_ref`: `gh-498,gh-126`.

## Scope and owned paths

This sibling owns close and stack mechanics only:

- `scripts/sanity-stack-link` and `scripts/atm-task-close.py`;
- `scripts/tests/test_sanity_stack.py` and `test_task_close.py`.

It does not own Jev requests, JSONL, record/history templates, or prompt tests;
those are bl-5.

## Deliverables and deterministic criteria

- A not-reproducible close records checked evidence without inventing a fix.
- A controlled rebase-required result links and reports mechanically. D3's
  reopen decision remains open; this does not decide it.
