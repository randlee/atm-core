# Sprint bl-12 — Jev and Quality uniform integrations

closure_type `integration`; target_boundary `BOUNDARY-JevResult` and `BOUNDARY-QaReviewers`. Branch `sprint/bl-12-uniform-integrations`, PR → `sprint/bl-9-refresh-integration`; Wave 3.

## Goal
Prove the cross-boundary Jev and Quality contracts end to end without editing producer paths.

## Owned paths
New `tests/test_jev_uniform.py`, `tests/fixtures/jev_uniform/`, and a new plugin fixture directory only.

## Relations
`must_follow` bl-4 for `metadata.reviewers`, bl-5 for sanity JSONL/D5, and bl-7 for console rendering. `parallel_safe`: bl-9. This PR targets the higher bl-9 layer after merge-forwarding the required producer heads.

## Deliverables and acceptance
- [ ] For every `JevErrorKind`, a real client JSONL error fixture travels through sanity history and renders the console do-not-enter icon; D5 selection is asserted.
- [ ] A quality-manager `metadata.reviewers` example is read by the Quality pane unchanged.

## Validation
Deterministic fixture tests and `just lint`.
