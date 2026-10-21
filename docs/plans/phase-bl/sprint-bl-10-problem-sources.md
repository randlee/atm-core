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
the persisted problem-source records.

## Deliverables, acceptance, and validation

Collectors retain latest non-cancelled check selection, branch fallback,
sprint labels and anchor exclusion. Derivation reads current typed records and
never triggers reads/sends. Controlled fixtures prove superseded checks cannot
win, independent source errors/rates, and exact actionable fields. Run plugin
boundary tests and lint; no live source calls.
