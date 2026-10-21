# Sprint bl-10 — problem collectors and state derivation

`BOUNDARY-ProblemSources`; wave 1; consumes bl-1's published collector-record
types only, not its collector implementation. GitHub, BD, and atm-bd source
modules collect independently; state derivation produces typed actionable
problems from current records without triggering reads or messages. Retain
latest non-cancelled check selection, branch fallback, sprint labels, and
`--exclude-label anchor`; fixtures construct the published record types to
prove independent failure/rate state and exact derived fields.

## Relations, scope, and owned paths

The bl-1 relation is type-only; it is not an implementation edge. Own
`model.py`, `github_source.py`, `atm_bd.py` and their tests. bl-3 consumes
the persisted problem-source records. `must_follow`: none; `parallel_safe`:
all wave-1 sprints. This sprint owns no dispatch/delivery, collector type
definition, or console-rendering paths.

## Governing text (verbatim)

> "multiple collectors each in separate module governed by it's own rules (i.e. gh rate limited, dolt timeouts, atm no limits today). state evaluation based on current state. specific state transitions would trigger 'atm message request'."

> "if you don't have valid data, don't force an error."

## Public contract and deliverables

Collectors retain latest non-cancelled check selection, branch fallback,
sprint labels and anchor exclusion. Derivation reads current typed records and
never triggers reads/sends. Controlled fixtures prove superseded checks cannot
win, independent source errors/rates, and exact actionable fields. Run plugin
boundary tests and lint; no live source calls.

`derive_problem_records(records: Sequence[CollectorRecord[SourceValue]]) -> Sequence[ActionableProblem]`
is pure over current persisted records: `ActionableProblem` carries the
selected check, resolved branch, sprint labels, and typed source status.

## Deterministic acceptance and validation

- [ ] `test_latest_non_cancelled_check_per_name_wins`: controlled source rows prove cancelled and superseded checks cannot become the source record.
- [ ] `test_branch_falls_back_from_problem_to_sprint_container`: fixtures yield the exact typed branch when the direct field is absent.
- [ ] `test_sprint_labels_and_anchor_exclusion_are_preserved`: an injected BD payload retains labels while `--exclude-label anchor` removes only anchors.
- [ ] `test_independent_source_failure_is_typed_not_forced_problem`: a failed GitHub/BD/ATM record remains an explicit source failure, not invented data.
- [ ] `test_derivation_uses_current_records_without_read_or_send`: fake adapters prove derivation has no collection or delivery side effect.

```bash
cd plugins/atm-bd-orchestration && python3 -m pytest -q -rs
```

Tests use controlled source records and fakes only; no live sources, sleeps, load, or timing-dependent behavior.

## Non-closure

Does not implement bl-1 collector infrastructure, debounce or deliver event requests, or render problem rows. It publishes persisted problem-source input for bl-3 and later display consumers.
