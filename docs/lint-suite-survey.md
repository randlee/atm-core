# Lint And Architecture Visualization Survey

Status: draft

Input reviewed:

- `/Users/randlee/Downloads/arch-boundary-enforcement.md`
- current `integrate/phase-R` lint implementation
- current boundary enforcement and boundary documentation model

This document compares the reference recommendations against the current repository
implementation and classifies them into:

- adopt
- overlaps with current tools
- defer
- reject

It is a planning companion to `docs/lint-suite-prd.md`.

## Executive Summary

The reference document is directionally strong, especially on:

- separate latency tiers for lint
- architecture-focused visualization
- dependency-cycle tooling
- unsafe-surface visualization
- keeping enforcement machine-checked

The main mismatch is implementation style, not intent.

The reference assumes:

- centralized spec files
- `xtask`-style Rust implementation
- `syn`-driven AST enforcement

Current repo reality is:

- crate-local `docs/*/boundaries.md` as the boundary source of truth
- Python-based repo-local lint implementation
- a `just`-driven portable wrapper layer already merged into Phase R
- repo-specific policy now expected to live in TOML config rather than in Python code

Recommended direction:

- keep the current implementation foundation
- adopt the product and tooling ideas that clearly add value
- avoid rewriting the boundary foundation during the current phase
- continue requiring repo-specific policy to come from configuration or workspace
  discovery, not hard-coded script literals

## Adopt

### 1. `just lint fast`

Adopt.

Why:

- strongest local UX improvement from the reference document
- aligns with agent/developer workflows
- implementable without rewriting the current suite

Recommended initial contents:

- `fmt check`
- `boundaries`
- `manifests`
- `version`
- `identities`
- `lines`
- `pytests`

Do not include initially:

- `clippy`
- `cargo-deny`
- `cargo-shear`

### 2. `cargo-modules`

Adopt.

Why:

- best immediate architecture visualization candidate
- useful for module-structure inspection
- useful for dependency-cycle inspection
- low custom-development cost relative to value

Recommended use:

- `just view modules`
- possible later full-lint / CI `--acyclic` gate

### 3. `cargo-geiger`

Adopt.

Why:

- directly supports architecture visualization of unsafe ownership
- useful before we decide whether unsafe budgets become hard gates

Recommended use:

- `just view unsafe`

### 4. boundary coverage reporting

Adopt.

Why:

- builds directly on current boundary records
- provides architectural value rather than lint-only value
- low conceptual conflict with current tools

Recommended use:

- `just view boundaries`

### 5. consolidated `view` surface

Adopt.

Why:

- creates a clear separation between enforcement and architecture observability
- gives a stable home for module/dependency/unsafe/boundary outputs

Recommended initial targets:

- `view boundaries`
- `view modules`
- `view unsafe`
- `view lines`

## Overlaps With Existing Tools

### 1. machine-checked boundary enforcement

Already present.

Current implementation already provides:

- schema enforcement
- dependency edge enforcement
- external reference enforcement
- active implementation privacy checks
- test-bypass checks

The reference recommendation is valuable, but it overlaps with the current boundary
checker rather than replacing a missing capability.

### 2. repo-owned policy wrappers

Already present.

Current suite already wraps and normalizes:

- `cargo-deny`
- `cargo-shear`
- `codespell`
- formatting
- lint orchestration

### 3. architecture-oriented reporting

Partially present.

We already have:

- boundary inventory logging
- line-count reports
- fixture/test inventory logs

What is missing is the dedicated `view` surface, not reporting from scratch.

## Defer

### 1. `dep-insight`

Defer until after first `view` targets land.

Why:

- promising for crate dependency visualization
- but not required to start the architecture-visualization surface
- value should be proven against simpler graph/report outputs first

### 2. `cargo audit`

Defer.

Why:

- advisory overlap with `cargo-deny`
- not architecture visualization
- not needed to unlock current planning goals

### 3. `xtask check-dep-rules`

Defer.

Why:

- current boundary checker already enforces some dependency rules
- we should extend the current Python implementation before adding a second dependency
  rule engine

## Reject For Current Phase

### 1. centralized `spec/*/boundaries.toml`

Reject for current Phase R work.

Why:

- current boundary source of truth is already merged and documented as crate-local
  `docs/*/boundaries.md`
- introducing a second source of truth would create synchronization and migration churn
- the current format is already feeding enforcement successfully

### 2. full rewrite to `xtask` + `syn`

Reject for current phase.

Why:

- current Python implementation is functioning
- the rewrite cost is high
- the current limitation is product surface, not parser viability

This can be revisited later if:

- Python parsing becomes too brittle
- performance becomes a real problem
- we need Rust-native AST precision beyond what the current approach can support

### 3. RDF / SPARQL boundary analysis

Reject for current phase.

Why:

- too heavy for the current repo needs
- not necessary to unlock enforcement or visualization

## Recommended Near-Term Implementation

### R.2A candidates

Recommended for the next near-term sprint:

1. add `just lint fast`
2. add `just view`
3. add `just view modules`
4. add `just view unsafe`
5. add `just view boundaries`
6. add `just view lines`

This gives:

- the desired command surface
- immediate architecture visualization value
- limited custom-development burden

### Follow-on sprint candidates

Recommended after the first view slice is working:

1. consolidated static site under `artifacts/view/`
2. optional crate dependency visualization tool integration
3. boundary coverage/unspecced-item refinement
4. possible `cargo-modules --acyclic` CI/full-lint gate

## Minimal First Implementation Recommendation

If the goal is “let us see outputs quickly with little custom development”, the first
implementation slice should be:

- `view modules`
  - raw `cargo-modules` structure/dependency output
  - DOT/SVG if available
- `view unsafe`
  - raw `cargo-geiger` text + JSON outputs
- `view boundaries`
  - current boundary inventory / coverage-oriented report

This is the highest-value low-custom-dev set.

## Decision Guidance For Discussion

Recommended default answers unless redirected:

- keep boundary enforcement in `crates/atm-architecture`: yes
- move to centralized `spec/*`: no
- add `lint fast`: yes
- add `cargo-modules`: yes
- add `cargo-geiger`: yes
- add `dep-insight` immediately: no
- add `cargo audit` immediately: no
- build static site immediately: not before the first raw-artifact view targets
- use the upcoming XHTML / template pattern for custom HTML: yes

## Suggested Next Decisions

The main remaining product decisions are:

1. which first `view` targets land now
2. whether `cargo-modules --acyclic` is view-only first or also a lint gate
3. whether `view deps` is deferred until after `view modules` and `view unsafe`
4. whether the first `view` implementation ships as raw artifacts only, or with a small
   transitional index page before the final XHTML-based site shell
