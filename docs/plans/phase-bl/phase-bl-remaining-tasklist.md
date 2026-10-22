# Phase BL — remaining delivery checklist

This is a branch-local execution checklist for the remaining Phase BL plan work.
It does not replace `phase-bl-plan.md` or `phase-bl.jsonl`.

## Consistency closure

- [x] Complete the typed versioned contract: phase identity, previous-state semantics, held rows, QA/Jev/DAG inputs, consumer doubles, and codec fixtures.
- [x] Specify the JSON/CLI plugin-to-console enqueue request, acknowledgement, and error response.
- [x] Make the held-lock policy uniform: return persisted/in-progress state, one source query, and no wait wording.
- [x] Specify durable delivery identity, due/attempt state, typed send outcome, retry, restart recovery, and acknowledged-success deduplication.
- [x] Complete the production cutover of `cli.py`, `app.py`, `runner.py`, and `model.py`; installed cron/show/TUI must use the separated pipeline.
- [x] Complete #347: bl-4 producer metadata, bl-7 two-panel selection/Markdown UI, bl-12 composed feature closure.
- [x] Verify `.sprints/BL/structure.ttl` exactly matches the 12 JSONL sprint IDs, docs, branches, and dependencies.

## Critical review gates

- [x] Every sprint header names the implementation repository (`randlee/atm-bd-orchestration`), `integrate/phase-bl` target, and validation worktree.
- [x] Every listed deliverable has a named deterministic test; no test depends on time, sleeps, races, load, event ordering, or live services.
- [x] Every boundary test consumes bl-1 fixtures rather than inventing payload shapes.
- [x] Validate JSONL dependencies, registry set/dependencies, header consistency, and `git diff --check`.
- [x] Push the complete correction set; send the commit report, then validation report; close only with the required consistency template.
