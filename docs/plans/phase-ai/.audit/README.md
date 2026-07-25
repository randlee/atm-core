# Phase AI audit guidelines

## Time window

- **AICH work start:** `2026-07-25T04:15:13Z`, the timestamp of the first
  `AICH-S1` assignment in `.sprints/AICH/events.ttl`.
- **Audit-session start:** `2026-07-25T16:25:02Z`, the first machine context
  query for this audit.
- Git and ATM queries about sprint work should use the AICH work-start time as
  their default lower bound. Use the audit-session start only when measuring
  actions taken during this audit.
- Always state the UTC lower/upper bounds and the branch/ref queried. Do not
  mix pre-window Phase AI history into AICH conclusions unless it is explicitly
  tied to AICH-S1 through AICH-S10.

## Scope

This audit covers only the sprint rows supplied for AICH:

| AICH sprint | Phase sprint |
|---|---|
| AICH-S1 | AI.21-pre |
| AICH-S2 | AI.22 |
| AICH-S3 | AI.23 |
| AICH-S4 | AI.24 |
| AICH-S5 | AI.25 |
| AICH-S6 | AI.26 |
| AICH-S7 | AI.27 |
| AICH-S8 | AI.28 |
| AICH-S9 | AI.30 |
| AICH-S10 | AI.29 |

Exclude unrelated Phase AI sprints, findings, branches, and ATM messages.

## Evidence rules

1. Treat the supplied sprint table as the status snapshot and `.sprints/AICH`
   as the event ledger; report discrepancies rather than silently reconciling
   them.
2. Record the exact command, branch/ref, UTC window, and output summary for
   every graph-orchestration or triage script run.
3. Keep the audit worktree read-only for product code. Only audit artifacts
   under this `.audit/` directory may be added or changed.
4. Distinguish a script result from a valid conclusion. In particular, verify
   that findings are linked to the sprint IRIs and timestamps expected by the
   graph-orchestration queries before trusting `DONE`, `CLEANUP`, or cursor
   output.
5. Compare `develop` and `integrate/phase-AI` at pinned commit SHAs. Note
   scripts present only on one ref and fixes that have not been merge-forwarded.

## Current audit baseline

- Audit branch: `audit/phase-ai`
- Base: `integrate/phase-AI`
- Integration HEAD at setup: `8627d5f3628e5ebd3bf271b3ac5b7ccf345dc652`
- Local `develop`: `643fe719ac5265e4b58d6628c771aad850ba156f`
- `origin/develop`: `e31af4f8107902464ab00c48eab8e2bfa37fffe3`

