# Sprint bl-10 — problem source accuracy and labels

closure_type `boundary`; target_boundary `BOUNDARY-ProblemSources`. Branch `sprint/bl-10-problem-sources`, PR → `sprint/bl-8-ruled-console-fixes`; Wave 1; no dependency.

## Goal
Deliver #375, #372 and #359 at production quality without sharing dispatch paths.

## Owned paths
`model.py`, `github_source.py`, `atm_bd.py`; `tests/test_model.py`, `tests/test_github_source.py`, `tests/test_atm_bd.py`.

## Deliverables
- Reduce check runs to the latest non-cancelled run per check name.
- Resolve a problem's branch from the bead or its sprint container and render typed, actionable problem text.
- Add sprint labels to the model data and `--exclude-label anchor` to bd reads.

## Relations
`parallel_safe`: all wave-1 sprints. `must_follow`: none. Every owned path is disjoint from bl-3.

## Acceptance
- [ ] Source fixtures prove cancelled/superseded check runs never win.
- [ ] Branch fallback and sprint-label fixtures prove exact rendered fields.

## Validation
Plugin boundary tests and `just lint`.
