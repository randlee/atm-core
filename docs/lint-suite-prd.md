# Lint And Architecture Visualization PRD

Status: draft, authoritative for current lint/tooling inventory in `plan/pR-lint-survey`

## Purpose

This document is the product requirements baseline for the repository lint suite and the
future architecture-visualization suite.

It serves two purposes:

1. Document what exists today on `integrate/phase-R`.
2. Define the command and product surface that future lint and visualization work must
   align to.

This document is authoritative for the tool UX and current-state inventory. It is not yet
the final implementation plan for every future lint or visualization tool.

## Scope

In scope:

- repo-local `just` command surface
- custom Python lint tools under `.just/`
- external lint tools invoked by repo wrappers
- CI wiring for the lint suite
- architecture-boundary enforcement currently driven by `docs/*/boundaries.md`
- future architecture-visualization tool surface

Out of scope:

- Rust production architecture itself
- full boundary rule definitions already documented in crate-local boundary docs
- implementation details of future visualization generators beyond command-level product
  requirements

## Product Principles

- Lint is a gate. It answers pass/fail questions.
- Visualization is architectural observability. It helps humans inspect system shape,
  dependency structure, boundary coverage, unsafe concentration, and module graphs.
- Visualization is not “visualization of the lint suite”.
- Repo-local tooling must remain cross-platform.
- Custom lint logic must stay portable enough to reuse in other Rust repositories.
- Custom Python lint tools must have unit tests.
- Passing command output should stay compact.
- Failure output should remain concise on stdout/stderr and write full transcripts to log
  files.

## Current Implementation

### Current command surface

The current `Justfile` exposes:

- `just help`
- `just build`
- `just test`
- `just clean`
- `just version`
- `just version latest`
- `just fmt`
- `just fmt check`
- `just fmt write`
- `just fmt apply`
- `just lint`
- `just lint fmt`
- `just lint clippy`
- `just lint deny`
- `just lint shear`
- `just lint boundaries`
- `just lint manifests`
- `just lint version`
- `just lint identities`
- `just lint lines`
- `just lint spell`
- `just lint pytests`
- `just view`
- `just view boundaries`
- `just view modules`
- `just view deps`
- `just view unsafe`
- `just ci`

Current behavior:

- `just lint` runs the full repo lint suite.
- `just lint <tool>` runs one lint target.
- `just version` reports the current workspace version state.
- `just version latest` reports recommended direct dependency upgrades per workspace member.
- `just view` runs the implemented architecture-view targets.
- `just view <tool>` runs one architecture-view target.
- `just fmt` is non-mutating and behaves like `just fmt check`.
- `just ci` runs `just lint` followed by `just test`.

### Current lint orchestrator

The primary lint entrypoint is:

- `.just/run_lint.py`

Current execution model:

- Cargo-based lints run sequentially.
- Python/read-only lints run in parallel.
- Each lint writes its own log file under `.just/logs/`.
- The umbrella runner prints short pass/fail summaries.

### Current custom Python lint tools

- `.just/check_line_counts.py`
  - enforces `RULE-003`
  - reports per-file and per-crate totals
  - distinguishes `total`, `prod`, `test`, and `prod+test`
- `.just/check_test_identity_literals.py`
  - enforces `RULE-008` and `RULE-009`
  - supports comment-based suppression directives
- `.just/check_version_sync.py`
  - verifies workspace/crate/release-version alignment
- `.just/lint_boundaries.py`
  - parses crate-local boundary records from `docs/*/boundaries.md`
  - validates schema and enforces current boundary rules
- `.just/lint_manifests.py`
  - enforces Cargo manifest policy beyond Cargo/Clippy defaults
- `.just/lint_cargo_deny.py`
  - wraps `cargo-deny`
- `.just/lint_cargo_shear.py`
  - wraps `cargo-shear`
  - promotes selected warnings to repo-policy failures
- `.just/lint_codespell.py`
  - wraps `codespell`
- `.just/run_fmt.py`
  - portable formatting entrypoint
- `.just/run_pytests.py`
  - Python lint-tool test runner with fixture inventory logging
- `.just/run_view.py`
  - architecture-view orchestrator
- `.just/view_boundaries.py`
  - emits boundary inventory/coverage artifacts
- `.just/view_modules.py`
  - emits raw `cargo-modules` structure/dependency artifacts
- `.just/view_unsafe.py`
  - emits raw `cargo-geiger` unsafe-surface artifacts
- `.just/lint_common.py`
  - shared helper surface for logs, tables, workspace crate inventory, and directive
    parsing

### Current external lint tools

Current external tools wired into the repo:

- `cargo fmt`
- `cargo clippy`
- `cargo-deny`
- `cargo-shear`
- `codespell`
- `cargo-modules`
- `cargo-geiger`
- `dep-insight`

Not currently wired:

- `cargo-audit`

### Current architecture-boundary enforcement

Current source of truth:

- `docs/atm/boundaries.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`

Current enforcement approach:

- boundary records live in fenced YAML inside Markdown docs
- historical note: the original boundary checker parsed those records
  directly; Phase AA superseded that implementation with the Rust
  `crates/atm-architecture/` enforcement crate
- enforcement is repo-local and cross-platform

Current boundary checks include:

- schema/required-field validation
- duplicate boundary id/name checks
- document-path owner consistency
- `owner_package` and `owner_crate_path` consistency checks
- `allowed_dependents` manifest enforcement
- forbidden manifest edges
- forbidden external references
- composition-root exemptions
- owner-crate test-bypass checks
- active-only implementation privacy / constructor / re-export checks

### Current logs and reporting

Current log design:

- one log file per lint run under `.just/logs/YYYYMMDDHHMMSS-<lint>.log`
- compact console output
- detailed transcripts in the log

Current log inventory behavior:

- crate-scoped lints log a `crates analyzed` table
- boundary logs also log `boundary docs analyzed`
- pytests log discovered test fixtures and counts
- version logs include the resolved workspace version

### Current repo-policy configuration

Repo-specific policy is expected to live in:

- `.just/lint-config.toml`

Current config-owned policy includes:

- forbidden production identity literals
- line-count limits and temporary exclusions
- boundary-doc discovery and boundary special-case ownership rules
- version-sync artifact wiring such as Winget and release checks

Current Python tooling derives workspace membership from the root `Cargo.toml` and reads
repo-specific policy from TOML rather than hard-coding repo names, crate names, or
artifact paths in the scripts.

### Current CI wiring

Current CI workflow:

- `just lint` runs on `ubuntu-latest`, `macos-latest`, and `windows-latest`
- separate `fmt`, `clippy`, and `test` jobs also exist
- CI installs:
  - Rust toolchain
  - Python
  - `just`
  - `cargo-deny`
  - `cargo-shear`
  - `codespell`

## Current Gaps

### Command surface gaps

The current command surface does not yet match the intended long-term UX.

Current gaps:

- no `just lint fast`

### Architecture visualization gaps

Current implementation now has an early architecture-visualization surface, but it is not
yet the final product.

Today we have:

- `just view boundaries`
- `just view modules`
- `just view deps`
- `just view unsafe`

We do not yet have dedicated architecture-visualization commands for:

- crate dependency graphs
- a consolidated static site
- XHTML/template-based custom HTML
- a stable `view unsafe` implementation for the current workspace/toolchain
- a final dependency-cycle policy or gate

### Product-definition gaps

Before this document, there was no single authoritative product description for:

- the lint suite command model
- the role split between lint and visualization
- the inventory of current tools
- the target UX for future visualization tooling

## Required Command Model

The intended long-term command surface is:

- `just lint`
- `just lint fast`
- `just lint <tool>`
- `just view`
- `just view <tool>`

Definitions:

- `just lint`
  - full local enforcement suite
- `just lint fast`
  - low-latency subset for local development and agent loops
- `just lint <tool>`
  - one enforcement tool only
- `just view`
  - run all enabled architecture visualization/report generators
- `just view <tool>`
  - run one visualization/report generator only

## Lint Product Requirements

- The lint suite must remain cross-platform on macOS, Linux, and Windows.
- The lint suite must support both full and fast modes.
- All repo-owned Python lint tools must have unit tests.
- Lint tools must use compact console output and full transcript logs.
- Repo-specific policy must be configurable rather than buried in code where practical.
- Boundary enforcement must remain machine-checked on every relevant lint run.

## Architecture Visualization Product Requirements

- Visualization must focus on architecture, not lint internals.
- Visualization commands must not mutate source files.
- Visualization commands may generate artifacts and reports.
- Visualization commands should succeed even when the architecture being visualized is
  imperfect, unless generation itself fails.
- Visualization tooling should prefer existing ecosystem tools where they are already good
  enough.

Target visualization categories:

- boundary inventory and coverage
- module dependency structure
- crate dependency graphs
- unsafe concentration / unsafe ownership
- cycle detection / dependency health

## Initial View Targets

The likely first `view` targets are:

- `just view boundaries`
  - boundary inventories, coverage, and rule summaries
- `just view modules`
  - module graph / cycle visualization
- `just view deps`
  - crate dependency graph
- `just view unsafe`
  - unsafe concentration report
- `just view lines`
  - file/crate size report

These are targets, not all currently implemented commands.

## Decision Record

Decisions captured here:

- boundary visualization is architecture visualization
- lint and view are distinct product surfaces
- current authoritative boundary source remains `docs/*/boundaries.md`
- current boundary enforcement remains the repo-local Python implementation unless and
  until explicitly replaced

## Next Step

The next step after this baseline document is a survey/review document that compares the
reference recommendations against this current implementation and classifies them into:

- adopt
- overlaps with current tools
- defer
- reject

## Recommended Design

This section finalizes the recommended product shape so implementation can begin.

### Authoritative sources

Authoritative for lint and visualization product UX:

- this document

Authoritative for current architectural boundary rules:

- `docs/atm/boundaries.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`

Authoritative for repo-local lint implementation details until replaced:

- `Justfile`
- `.just/*.py`
- `.github/workflows/ci.yml`

### Final command model

The intended command surface is:

- `just lint`
- `just lint fast`
- `just lint <tool>`
- `just view`
- `just view <tool>`
- `just build`
- `just test`
- `just clean`
- `just fmt`
- `just fmt check`
- `just fmt write`
- `just fmt apply`

Rules:

- `lint` commands are enforcement gates.
- `view` commands are architecture visualization or architectural report generators.
- `view` commands are not lint aliases.
- `view` commands must not mutate source.
- `view` commands may generate artifacts, static pages, graphs, and reports.

### Final lint mode definitions

#### `just lint`

Full local lint suite.

Includes:

- formatting gate
- clippy gate
- boundary enforcement
- manifest policy
- version alignment
- line-count policy
- identity-literal policy
- spelling/content checks
- Python lint-tool unit tests
- external dependency-policy tools that are acceptable in local full runs

#### `just lint fast`

Low-latency developer/agent lint subset.

Design goals:

- target local turnaround in roughly 10–30 seconds on a warm workspace
- catch syntax/configuration/policy regressions early
- avoid slow advisory/network-heavy or compile-heavy checks where possible

Recommended initial `lint fast` contents:

- `just fmt check`
- `just lint boundaries`
- `just lint manifests`
- `just lint version`
- `just lint identities`
- `just lint lines`
- `just lint pytests`

Recommended initial exclusions from `lint fast`:

- `clippy`
- `cargo-deny`
- `cargo-shear`
- any future heavy visualization generators

#### `just lint <tool>`

Single-tool enforcement entrypoint.

Design rules:

- every individual lint tool must remain directly runnable
- tool-specific logs must still be produced
- output must remain compact

### Final architecture-visualization model

Architecture visualization is a separate product surface from lint.

Its purpose is to help humans inspect:

- crate dependency structure
- module dependency structure
- boundary inventory and coverage
- unsafe concentration / unsafe ownership
- dependency-cycle health
- file/crate size distribution when relevant to architecture work

Visualization does not mean:

- visualizing the lint suite itself
- mirroring lint log files directly as the primary experience

### Final `view` targets

The first supported `view` targets should be:

- `just view boundaries`
- `just view modules`
- `just view deps`
- `just view unsafe`
- `just view lines`

Definitions:

- `just view boundaries`
  - boundary inventory
  - per-doc boundary counts
  - boundary coverage / unspecced-public-item reporting if added
  - current enforcement status summary
- `just view modules`
  - module dependency structure
  - cycle-oriented views
  - module graph rendering
- `just view deps`
  - crate-level dependency graph
  - optional interactive HTML if the chosen tool supports it well
- `just view unsafe`
  - unsafe counts and ownership concentration
  - per-crate unsafe surface
- `just view lines`
  - current line-count tables rendered as a human-facing report

### Static site requirement

`just view` should generate a consolidated static site.

Required shape:

- one output root, e.g. `artifacts/view/`
- one top-level `index.html`
- one page per view target
- shared navigation across pages
- generated artifacts stored under predictable subdirectories

Desired initial structure:

- `artifacts/view/index.html`
- `artifacts/view/boundaries/index.html`
- `artifacts/view/modules/index.html`
- `artifacts/view/deps/index.html`
- `artifacts/view/unsafe/index.html`
- `artifacts/view/lines/index.html`

Design rules:

- prefer static HTML outputs
- wrap non-HTML outputs in generated HTML pages where needed
- treat tool output as data or embedded artifact, not as the final UX

Transitional allowance:

- before the shared XHTML/template pattern is available, initial `view` implementations
  may emit raw artifacts plus a simple artifact index
- the final product target remains a consolidated static site

### HTML generation direction

When custom HTML is implemented:

- prefer shared template-driven page generation
- keep pages composable and loop-friendly
- use a common site shell/navigation
- do not hardcode page-specific HTML in multiple scripts

The exact template system can follow the XHTML / `sc-compose` / Jinja pattern once the
user provides the concrete reference path.

## Tool Adoption Decisions

### Adopt now

These should be incorporated into the planned suite.

#### `cargo-modules`

Adopt for architecture visualization and probably for CI cycle checking.

Planned uses:

- `just view modules`
- possible CI/full-lint cycle gate via `--acyclic`

Reason:

- directly aligned with architecture visualization goals
- produces dependency structure rather than lint-internal noise

#### `cargo-geiger`

Adopt for `just view unsafe`.

Possible later lint use:

- CI/full-lint policy gate if unsafe concentration becomes explicitly budgeted

Reason:

- directly supports architecture visualization of unsafe ownership
- useful even before it becomes an enforcement gate

#### Boundary coverage reporting

Adopt as an extension of the existing boundary checker and/or `view boundaries`.

Reason:

- builds directly on current `docs/*/boundaries.md`
- avoids unnecessary rewrite
- gives a strong architectural view of what is and is not yet constrained

#### `just lint fast`

Adopt now as a product-surface addition.

Reason:

- strong local/agent UX improvement
- aligns with the reference document
- can be implemented without rewriting the current suite

### Adopt, but only as full/CI lint

#### `cargo-deny`

Already adopted.

Decision:

- keep it in full lint / CI
- do not move it into `lint fast`

#### `cargo-shear`

Already adopted.

Decision:

- keep it in full lint / CI
- continue wrapper-managed repo-policy handling

### Likely adopt after initial implementation

#### `dep-insight` or equivalent crate-dependency visualizer

Adopt only if it materially improves crate-level architecture visualization beyond what
we can get from existing tooling or generated graphs.

Reason:

- promising for `just view deps`
- not required to start the visualization surface

#### `cargo audit`

Defer unless we want a lighter-weight advisory path alongside or instead of some
`cargo-deny` usage.

Reason:

- overlaps with advisory portions of `cargo-deny`
- not architecture visualization

### Defer

#### `xtask lint-boundaries` with `syn`

Do not adopt now.

Reason:

- the current Rust boundary enforcement crate is already working and
  cross-platform
- a rewrite would add churn without immediate product gain
- worth revisiting only if Python parsing/enforcement becomes limiting

#### `xtask check-dep-rules`

Defer.

Reason:

- the current Rust boundary checker already enforces some dependency rules from
  the boundary records
- we should first see whether extending `crates/atm-architecture` is
  sufficient

### Reject for current phase

#### Separate `spec/*/boundaries.toml` source of truth

Do not adopt now.

Reason:

- current authoritative format is crate-local `docs/*/boundaries.md`
- a second source of truth would create churn and synchronization burden
- the current doc-driven model is already merged into Phase R planning

#### RDF / SPARQL analysis

Reject for the current phase.

Reason:

- much too heavy for current needs
- not needed to unlock architecture visualization or repo-local enforcement

## Overlap Analysis

The reference document overlaps with current work in these areas:

- repo-local policy wrappers for Cargo and Python tools
- boundary enforcement as a machine-checked gate
- dependency rule enforcement
- architecture-oriented logs and reports
- low-latency vs full-latency lint thinking

The main difference is not direction, but implementation style:

- reference doc prefers `xtask` + TOML spec + `syn`
- current suite uses Python + crate-local boundary docs + repo-local wrappers

Decision:

- keep the current implementation direction
- adopt the useful product ideas without rewriting the foundation

## Implementation Plan

### Phase 1: Command-surface alignment

Required:

- add `just lint fast`
- add `just view`
- add `just view <tool>` targets
- update help output to document both surfaces

### Phase 2: First architecture-view implementations

Required:

- implement `just view boundaries`
- implement `just view lines`
- prototype `just view modules`
- prototype `just view unsafe`

Preferred output:

- static site under `artifacts/view/`

### Phase 3: External architecture visualization tools

Required:

- integrate `cargo-modules`
- integrate `cargo-geiger`

Optional depending on value:

- integrate `dep-insight` or an alternative dependency visualizer

### Phase 4: Coverage and refinement

Required:

- add boundary coverage reporting
- refine page structure/navigation
- align CI and artifact generation where appropriate

## Sprint Recommendations

### R.2A candidates

Best candidates for `R.2A` parallel lint hardening:

- `just lint fast`
- `just view boundaries`
- `just view lines`
- command/help surface refactor
- architecture-visualization output directory/site shell

### New sprint candidates

Worth a dedicated new sprint if `R.2A` is already full:

- `cargo-modules` integration
- `cargo-geiger` integration
- consolidated static architecture site
- optional crate dependency visualization tooling

## Non-Goals

- rewriting the boundary suite into Rust immediately
- replacing crate-local boundary docs with centralized spec files
- turning visualization into a mirror of lint logs
- requiring every proposed external tool to become a gate immediately

## Ready-to-Implement Conclusion

This design is ready for implementation with the following immediate priorities:

1. Add the missing command surface:
   - `just lint fast`
   - `just view`
   - `just view <tool>`
2. Implement first-party architecture views:
   - `view boundaries`
   - `view lines`
3. Add ecosystem architecture visualization tools:
   - `cargo-modules`
   - `cargo-geiger`
4. Generate a consolidated static site under one output root.

That path preserves the current working lint foundation while moving directly toward the
architecture-visualization UX you want.
