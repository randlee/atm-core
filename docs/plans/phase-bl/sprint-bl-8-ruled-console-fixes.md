# Sprint bl-8 — D1-gated console BD deadline

closure_type `boundary`; target_boundary `BOUNDARY-BdDeadline`. Branch
`sprint/bl-8-ruled-console-fixes`, PR → `sprint/bl-4-qa-reviewer-data`. Wave
1. `external_ref`: `gh-533`.

## Scope

This is a D1-gated, deadline-only sprint. Once Rand rules the console BD read
deadline, it changes only `src/atm_bd_console/command.py` and
`tests/test_command.py`. It has no dependency on bl-1 and makes no refresh,
runner, facts, notification, status-table, or merge-policy change.

## Deterministic criteria

- A fake clock and controlled command result prove the ruled deadline is
  passed to the command seam.
- A completion before that deadline is accepted and a controlled timeout is
  represented by the existing typed result.
- The test suite names the rule and value explicitly; it does not wait on
  wall time.

## Does not close

D2 / #492 remains open. “Head not pushed,” PR-less merge checks, facts
presentation, status-table changes, and all three formerly proposed #492
criteria receive a new sprint only after D2 is ruled.
