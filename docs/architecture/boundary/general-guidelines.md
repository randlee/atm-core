# Boundary Review Guidelines

This document defines the stable review rules for boundary enforcement in
`atm-core`. It is intentionally short and principle-based. Do not treat
historical incidents or branch-specific code shapes as the source of truth.

## Primary rule

Every important decision has exactly one owner.

If two modules can independently answer the same architectural question, the
boundary is already too wide.

Every meaningful production code path must also have a clear reason to exist.

If no requirement, ADR, or boundary rule justifies a path, that path is
presumed unnecessary until proven otherwise.

Examples:

- local vs cross-host routing
- send vs ack execution path
- endpoint resolution
- retry / deferred / terminal classification
- inbox mutation ownership
- storage schema interpretation

## Core review tests

Flag a finding when any of these are true:

- code exists with no clear retained justification
- the same behavior is implemented in more than one place
- two paths could be collapsed into one retained implementation
- the same decision logic is implemented in more than one place
- a caller receives raw inputs and re-decides a fact instead of consuming an
  already-resolved result
- a trait lives in the wrong crate and forces an unnecessary dependency edge
- a concrete backend type appears above the layer that owns it
- a component reaches around an existing boundary and calls a side channel
- a proof/test-only path exists in production code
- a state machine exists only to coordinate around another bad split

## Review priorities

Prioritize findings in this order:

1. duplicate production paths
2. unjustified code with no clear retained requirement
3. concrete implementation leakage above a trait or module boundary
4. visibility wider than needed
5. missing mechanical enforcement for a repeated leak pattern
6. stale docs or stale lint policy that no longer match the code

## Mandatory design constraints

- one owner per decision
- one production path per behavior
- every retained path must have explicit justification
- narrowest trait surface that works
- narrowest visibility that works
- no backend knowledge above the backend boundary
- no transport knowledge above the transport boundary except resolved facts
- no proof-specific production branch

## Mechanical enforcement expectation

Treat these as mandatory evidence:

- `boundaries/**/*.toml`
- `.just/lint_boundaries.py`
- `.just/lint_manifests.py`
- `.just/run_lint.py`
- `crates/atm-architecture/tests/boundary_enforcement.rs`
- `crates/sc-lint-boundary/config/defaults.toml`
- `docs/sc-lint/README.md`
- `docs/sc-lint/boundary-enforcement-model.md`
- `.claude/agents/boundary-guard.md`

If a repeated leak pattern is real but none of these can catch it, emit a
`lint_gap` finding.

## Output expectations

- cite the violated guideline first
- then cite the code location
- then state the narrowest fix
- prefer deletion over relocation
- prefer relocation over new wrapper code
