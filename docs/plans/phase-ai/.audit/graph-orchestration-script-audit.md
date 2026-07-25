# Graph-orchestration and triaging-findings script audit

Audit window: `2026-07-25T04:15:13Z` through `2026-07-25T16:40:12Z` UTC.
The lower bound is the first AICH event. Scope is AICH-S1 through AICH-S10 as
defined in [README.md](README.md).

## Refs compared

| Ref | SHA | Result |
|---|---|---|
| `origin/integrate/phase-AI` | `8627d5f3628e5ebd3bf271b3ac5b7ccf345dc652` | AICH event ledger; no graph-orchestration files |
| `develop` | `643fe719ac5265e4b58d6628c771aad850ba156f` | Graph scripts present; no ignore file |
| `origin/develop` | `e31af4f8107902464ab00c48eab8e2bfa37fffe3` | Graph scripts plus `.triage/.graph-orchestration-ignore` |

`git diff origin/integrate/phase-AI..origin/develop` shows the graph skill,
its SPARQL/Python scripts, tests, and the ignore file as additions. The
`triaging-findings` skill, `scripts/triage_carry_forward.py`, and its tests are
byte-identical on develop and integrate.

## Runs and outputs

### Graph-orchestration on AICH data

Command (using the develop script against the audit copy of the integrate
worktree):

```text
.claude/skills/graph-orchestration/scripts/next-dev-task AICH \
  /Users/randlee/Documents/github/atm-core-worktrees/audit/phase-ai/.sprints/AICH
```

Result: exit `0`, `37` malformed-findings warnings, and:

```json
{
  "phase": "TRAVERSAL",
  "vars": {
    "sprint": "AICH-S10",
    "sprint_iri": "urn:atm:triage:AICH-S10",
    "sprint_order": 10,
    "criteria_doc": "docs/plans/phase-ai/sprint-ai-29-crosshost-smoke-rerun.md"
  }
}
```

The documented `--validate-only` form returns the exact same cursor JSON and
warnings; the shell wrapper ignores all arguments after the first two, so it
does not implement a validation-only mode.

Direct query results on the same graph:

| Query | Rows | Rows returned |
|---|---:|---|
| `validate-structure.sparql` | 0 | no reported violations |
| `cursor.sparql` | 1 | `AICH-S10` |
| `all-complete.sparql` | 2 | `AICH-S8`, `AICH-S10` incomplete |
| `open-findings-sprint.sparql` | 0 | no cleanup findings |

Running with the `origin/develop` ignore list suppresses all 37 warnings and
produces the same `AICH-S10` cursor. The result is therefore stable, but the
integrate branch has avoidable warning noise.

Graph unit tests on develop: `18 passed`.

The same graph entrypoint cannot be run from `integrate/phase-AI` because
`.claude/skills/graph-orchestration/scripts/next-dev-task` is absent on that
ref.

### AICH validation

`validate-structure.sparql` was run against the AICH `structure.ttl` plus
`events.ttl` using the develop-side validator:

- SPARQL violations: `0`
- sprint count: `10`
- orders: contiguous `1..10`
- criteria paths: `10/10` exist

This supplemental criteria/order check is recorded because the validator query
itself only checks duplicate orders and missing `triage:criteria`/
`triage:order` properties; it does not check path resolution or contiguity.

### Triaging-findings carry-forward

Both refs pass the three unit tests in `scripts/test_triage_carry_forward.py`.
The carry-forward script produced identical output on both refs for the same
63 AICH finding records and branch `integrate/phase-AI`: one row,
`AI22-BLOCK-004`, at `crates/atm-daemon/src/runtime_health.rs:617`.

For comparison, querying the same records by
`feature/pAI-s25-peer-authority-resolution` returns 18 open occurrence rows,
and by `feature/pAI-s27-peer-delivery-observability` returns 9. This is
branch-specific behavior, not a complete phase finding inventory.

## Issues found

1. **High — graph cannot see the AICH findings.** The 63 scoped AICH finding
   files contain zero `triage:foundIn` links and use `triage:phaseId`,
   `triage:status`, and `triage:triagedAt` instead. `query_runner.py` only
   imports findings whose `triage:foundIn` points at a declared sprint, and the
   SPARQL queries require `triage:foundAt`. As a result, 39 raw open AICH
   findings are invisible: completion invalidation and CLEANUP detection both
   report clean.
2. **High — the integration ref cannot execute graph orchestration.** The
   skill and all graph scripts exist only on develop (and its descendants), not
   on `integrate/phase-AI`, even though the AICH event ledger lives there.
3. **Medium — `--validate-only` is documented but not implemented.** The
   examples in `SKILL.md` pass this flag, but `next-dev-task` consumes only
   `$1` and `$2` and silently ignores it.
4. **Medium — dependency name mismatch.** `graph-orchestration/SKILL.md`
   declares `triage-findings`, while the repository skill is named
   `triaging-findings`. Exact skill resolution may therefore fail.
5. **Medium — structure validation is incomplete.** The validator catches
   duplicate orders and missing properties, but does not catch non-contiguous
   sprint orders or nonexistent criteria files. A synthetic phase with orders
   1 and 3 and missing criteria paths returned zero violations.
6. **Low — warning suppression is not merge-forwarded.**
   `.triage/.graph-orchestration-ignore` is present on `origin/develop` but not
   on `integrate/phase-AI`; this causes 37 repeat warnings per cursor run.
7. **Record consistency — the supplied status table and event ledger differ.**
   The table says S7 is in progress and S8 is not started; the ledger contains
   `Completion AICH-S7` and `Assignment AICH-S8`, and the graph cursor skips the
   in-flight S8 and selects S10. Preserve both snapshots and resolve the
   discrepancy explicitly.

## Recommended next actions

1. Choose and document one canonical finding schema, or add an explicit
   adapter that maps each AICH finding to `AICH-S1`…`AICH-S10` plus a
   `foundAt` timestamp.
2. Merge/pin the graph skill on the integration branch before using its cursor
   as an AICH gate.
3. Add an AICH fixture to `test_queries.py` that contains a post-completion
   blocking finding and an open important finding; assert re-dispatch and
   CLEANUP behavior.
4. Implement and test `--validate-only`, and extend structure validation to
   enforce contiguous orders and resolvable criteria paths.
5. Record the status-table timestamp, commit/QA/CI evidence, and the rule for
   reconciling table state with `.sprints/AICH/events.ttl`.
