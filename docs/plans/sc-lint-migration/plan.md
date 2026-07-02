---
title: sc-lint Published Release Migration Inventory And Gap Analysis
plan_type: support_inventory
status: complete
branch: plan/sc-lint-published-migration
worktree: ../atm-core-worktrees/plan/sc-lint-published-migration
---

# sc-lint Published Release Migration Inventory And Gap Analysis

This document is retained as a supporting inventory and gap-analysis artifact.

The proposed execution planning package for this work now lives in:

- [`docs/plans/phase-AD/plan-phase-AD.md`](../phase-AD/plan-phase-AD.md)
- `docs/plans/phase-AD/sprint-AD1.md`
- `docs/plans/phase-AD/sprint-AD2.md`
- `docs/plans/phase-AD/sprint-AD3.md`
- `docs/plans/phase-AD/sprint-AD4.md`
- `docs/plans/phase-AD/sprint-AD5.md`
- `docs/plans/phase-AD/sprint-AD6.md`
- `docs/plans/phase-AD/sprint-AD7.md`
- `docs/plans/phase-AD/sprint-AD8.md`
- `docs/plans/phase-AD/sprint-AD9.md`

This supporting doc remains useful because it captures:

- the original vendored-vs-published inventory
- the initial gap analysis
- the early no-change verification for boundary TOMLs and release manifests
- the Python lint ownership review and duplicate-surface classification

## Goal

Remove `atm-core`'s vendored `sc-lint` crates and cut over to the next
published `sc-lint` release without weakening any current lint, CI, or release
gates.

This is planning only. No cutover begins until a published `sc-lint` version
exists that covers the current `atm-core` dependency and analyzer surface.

The inventory/gap-analysis task was the originally scoped deliverable for this
branch. The Phase `AD` phase/sprint package was added later as a planning
proposal after the user asked for a phase-structured execution plan. That
proposal is not an execution authorization by itself; it requires explicit
human sign-off before any `AD.*` implementation branch is opened.

## Baseline

- `atm-core` currently vendors these unpublished workspace members:
  - `crates/sc-lint-directives`
  - `crates/sc-lint-attributes`
  - `crates/sc-lint-boundary`
- The best available proxy for the next published release is the sibling
  `../sc-lint` repository, currently on workspace version `0.3.1`.
- The vendored ATM copies are not a trivial mirror of `../sc-lint`:
  - `sc-lint-boundary` differs by `16` files and a large rule-layout split
  - portability and runtime analyzers still live inside the vendored
    `sc-lint-boundary`
  - upstream has already split those families into:
    - `sc-lint-portability`
    - `sc-lint-runtime`
    - `sc-lint`
    - `sc-lint-schema`

## Non-Goals

- no immediate code migration in this sprint
- no promise to adopt a specific unpublished `sc-lint` commit
- no permanent dual-mode runtime that supports vendored and published analyzers
  indefinitely

## Current ATM Inventory

### A. Vendored crate and compile-time usage

| Current ATM asset | Current use | Load-bearing detail | Migration impact |
| --- | --- | --- | --- |
| root `Cargo.toml` workspace members | vendors `crates/sc-lint-directives`, `crates/sc-lint-attributes`, `crates/sc-lint-boundary` | the analyzers compile as part of the ATM workspace today | must be removed from the workspace during cutover |
| `crates/atm-core/Cargo.toml` | path dependency on `sc-lint-attributes = { path = "../sc-lint-attributes", version = "0.1.0" }` | compile-time proc-macro dependency | must move to a published registry dependency |
| `crates/atm-core/src/observability.rs` | uses `#[sc_lint(boundary.allow("cycle.recursive_value_container"))]` twice | ATM depends on the proc-macro compiling and preserving the current directive grammar | published `sc-lint-attributes` must remain source-compatible |

### B. Local lint wrappers and command entry points

| Current ATM asset | Current command shape | Current role | Migration impact |
| --- | --- | --- | --- |
| `.just/lint_sc_boundary.py` | `cargo run -q -p sc-lint-boundary -- analyze --root <repo> --format json` | manual full boundary analyzer wrapper | must stop calling the vendored workspace binary |
| `.just/lint_sc_portability.py` | `cargo run -q -p sc-lint-boundary -- analyze --root <repo> --rule portability --format json` | manual portability analyzer wrapper | must retarget to published portability surface |
| `.just/lint_unix_gating.py` | same portability command, then filter `PORT-004` and `PORT-005` | default-house lint wrapper for Unix gating | wrapper remains ATM-owned even after cutover |
| `.just/lint_runtime_waits.py` | `cargo run -q -p sc-lint-boundary -- analyze --root <repo> --rule boundaries --format json`, then filter `SCB-RUNTIME-001` / `SCB-RUNTIME-002` | default-house lint wrapper for production `Condvar` waits | must move to the published runtime analyzer surface |
| `Justfile` | `_lint-sc-boundary`, `_lint-sc-portability`, `_lint-unix-gating`, `_lint-runtime-waits` | user-facing local lint entrypoints | targets remain, backend commands change |
| `.just/run_lint.py` | `all` includes `unix-gating` and `runtime-waits`; `sc-boundary` and `sc-portability` stay manual extras | canonical local lint orchestrator | must preserve current lint names and failure formatting |

### C. Tests, CI, and release wiring

| Current ATM asset | Current use | Load-bearing detail | Migration impact |
| --- | --- | --- | --- |
| `.just/tests/test_lint_sc_boundary.py` | asserts the wrapper command uses `sc-lint-boundary` | wrapper-contract regression test | expected fixture updates |
| `.just/tests/test_lint_sc_portability.py` | asserts the portability wrapper command shape | wrapper-contract regression test | expected fixture updates |
| `.just/tests/test_lint_unix_gating.py` | asserts portability subset wrapper command shape | protects default-house lint contract | expected fixture updates |
| `.just/tests/test_lint_runtime_waits.py` | asserts runtime subset wrapper command shape | protects default-house lint contract | expected fixture updates |
| `.just/tests/test_run_lint.py` | asserts target names and task wiring | protects `just lint` surface | must stay green through retargeting |
| `.github/workflows/ci.yml` | runs `just lint` on Ubuntu, macOS, and Windows | current CI assumes analyzers are built from the workspace checkout | CI must install/pin published `sc-lint` tools first |
| `scripts/validate_release.py` | treats `just lint` or `.just/run_lint.py all` as a release gate | release sign-off depends on the lint suite | release preflight must use the published tool install path too |

### D. Python lint ownership review

| Current ATM asset | Current role | Keep or replace | Notes |
| --- | --- | --- | --- |
| `.just/run_lint.py` | local lint orchestration and house target naming | keep | ATM-owned wrapper/orchestration layer; not a `sc-lint` product concern |
| `.just/lint_sc_boundary.py` | thin full-boundary analyzer wrapper | replace backend | should call published `sc-lint-boundary` directly |
| `.just/lint_sc_portability.py` | thin portability analyzer wrapper | replace backend | should call published `sc-lint-portability` directly |
| `.just/lint_unix_gating.py` | repo-specific portability subset wrapper | keep | remains ATM-owned, but backend must come from published portability findings |
| `.just/lint_runtime_waits.py` | repo-specific runtime-waits subset wrapper | keep | remains ATM-owned, but backend must come from published `sc-lint-runtime` |
| `.just/lint_boundaries.py` | ATM boundary-TOML schema, allowlist, review-gate, and dependency-policy checks | keep partially | ATM-specific governance stays; dependency-policy overlap with released `sc-lint` `D.1` must be re-evaluated and reduced/deleted where redundant |

### E. Duplicate-surface classification

Duplicate implementation expected to be removed during execution:

- vendored crates:
  - `crates/sc-lint-directives`
  - `crates/sc-lint-attributes`
  - `crates/sc-lint-boundary`
- any analyzer invocation path that still relies on:
  - `cargo run -q -p sc-lint-boundary`
  - a workspace-built vendored `sc-lint` binary
- any ATM-local dependency-policy enforcement in `.just/lint_boundaries.py`
  that the released `sc-lint` `D.1` boundary scan proves equivalent or stronger

ATM-owned surface expected to remain after migration:

- repo-local lint names and orchestration in `.just/run_lint.py`
- subset wrappers for `unix-gating` and `runtime-waits`
- ATM-specific boundary schema / allowlist / review-gate checks in
  `.just/lint_boundaries.py`
- wrapper-contract tests that protect ATM report shape and target naming

## Vendored Snapshot vs Upstream Release-Line Gap

### ATM vendored snapshot facts

- vendored crate versions are still `0.1.0`
- all three vendored crates set `publish = false`
- vendored `sc-lint-boundary` still owns:
  - boundary rules
  - portability rules
  - runtime-wait rules
- vendored `sc-lint-boundary/config/defaults.toml` includes both:
  - boundary cycle ignore settings
  - portability `unix_path_prefixes`

### Upstream `../sc-lint` release-line facts

- workspace version is `0.3.1`
- upstream adds publishable crate packaging and release manifests
- upstream `sc-lint-boundary` is narrower:
  - boundary rules stay there
  - portability rules moved to `sc-lint-portability`
  - runtime rules moved to `sc-lint-runtime`
- upstream adds:
  - `sc-lint` top-level CLI
  - `sc-lint-schema`
  - dedicated analyzer READMEs and CLI contract docs
- upstream boundary defaults no longer carry the portability section, because
  portability moved out of the boundary crate

### Root-cause summary

`atm-core` is not merely "behind a version." It vendors an earlier integrated
`sc-lint` shape whose analyzer-family boundaries differ from the current
release line. The migration therefore requires both:

1. dependency cutover from path crates to published crates
2. wrapper/CI retargeting from one integrated analyzer binary to the released
   analyzer family surface

## Gap Analysis

| Current ATM requirement | Current ATM source of truth | Expected published `sc-lint` coverage | Risk / note |
| --- | --- | --- | --- |
| compile-valid `#[sc_lint(...)]` proc-macro support for `boundary.allow`, `boundary.internal_only`, `boundary.forbid_external_impls` | vendored `sc-lint-attributes` + `sc-lint-directives` | expected yes | low risk if the published proc-macro grammar stays source-compatible |
| boundary-cycle and boundary-visibility findings from `sc-lint-boundary analyze --format json` | vendored `sc-lint-boundary` | expected yes | moderate risk: ATM must verify JSON field compatibility, not just rule existence |
| graph export via `sc-lint-boundary export-graph` | vendored `sc-lint-boundary` | expected yes | low risk; no active ATM CI call site uses it today |
| portability findings on demand (`sc-portability`) | vendored `sc-lint-boundary --rule portability` | expected yes through `sc-lint-portability` | moderate risk because the analyzer owner and command path changed |
| default lint subset for `PORT-004` / `PORT-005` (`unix-gating`) | ATM wrapper filters portability findings | not a product feature by itself | ATM must keep this wrapper locally; retained `sc-lint` docs in this repo do not currently prove published `PORT-004` / `PORT-005` continuity, so `AD.4` must prove direct continuity or record an explicit upstream-to-ATM mapping |
| default lint subset for `SCB-RUNTIME-001` / `SCB-RUNTIME-002` (`runtime-waits`) | ATM wrapper filters boundary findings | semantic coverage expected through `sc-lint-runtime`, but rule-id continuity is unproven | moderate risk because ATM currently filters those IDs out of the wrong analyzer family and the released runtime analyzer may require an explicit upstream-to-ATM mapping |
| current `just lint` names and report formatting | `.just/run_lint.py` + local Python wrappers | not expected upstream | ATM-owned adapter layer must remain |
| dependency-policy enforcement from `dependencies.allowed_*` and `forbidden_edges` | partly duplicated in `.just/lint_boundaries.py` today | expected yes once released `sc-lint` Phase `D.1` lands | migration is not complete until ownership moves to released `sc-lint` or ATM explicitly proves why a residual check stays local |
| all-platform CI bring-up with no local vendored analyzer crates | workspace build today | unknown until release artifacts are published | high risk until install story is confirmed for Windows, macOS, and Linux |
| portability config knob `unix_path_prefixes` | vendored `sc-lint-boundary/config/defaults.toml` | uncertain | must confirm whether the published portability crate exposes an equivalent config surface or bakes the defaults in |
| release-preflight lint behavior | `scripts/validate_release.py` | not expected upstream | ATM-owned release gate must be retargeted |

The unresolved `unix_path_prefixes` portability-config gap is a hard planning
input for the later `unix-gating` cutover. The proposed execution package
assigns that closure explicitly to `AD.4`; no `unix-gating` sprint may claim
success until the published `sc-lint-portability` surface either provides an
equivalent knob or ATM carries the behavior forward in a documented wrapper-
owned override.

## Verified No-Change Areas

The cutover does not currently require a `boundaries/*.toml` or
`release/publish-artifacts.toml` edit.

Verification method used for this planning line:

- reviewed every current ATM boundary TOML with:
  - `find . -path '*/boundaries/*.toml'`
- confirmed the migration only changes:
  - proc-macro dependency sourcing
  - analyzer installation/invocation paths
  - local wrapper scripts
  - CI/release-preflight tool installation
- confirmed `release/publish-artifacts.toml` governs ATM release artifacts,
  not the repository-local lint backend choice, so the cutover does not add or
  remove ATM release binaries or crate publish targets

This assumption must be rechecked on the real cutover branch if the published
`sc-lint` install story forces a release-packaging change inside ATM itself.

## Required Migration Decisions

### Decision 1: Keep ATM wrappers, change only their backends

This is the lowest-risk cutover.

Reason:

- ATM's wrappers define repo-specific lint names and report formatting
- upstream `sc-lint` docs already classify wrappers such as
  `.just/lint_unix_gating.py` as consumer-repo adaptations rather than core
  product behavior
- preserving the ATM wrapper layer avoids forcing one release to absorb both a
  backend migration and a user-facing lint UX redesign

### Decision 2: Prefer published dedicated analyzers first, not the top-level CLI

Initial cutover target:

- `sc-lint-boundary`
- `sc-lint-portability`
- `sc-lint-runtime`
- published `sc-lint-attributes`
- published `sc-lint-directives`

Reason:

- ATM's current wrappers already think in analyzer-family terms
- the dedicated binaries are the closest semantic match to the current command
  shapes
- adopting the top-level `sc-lint --json` machine envelope at the same time is
  optional follow-up work, not required for the first safe migration

### Decision 3: Do not delete the vendored path until parity is proven

Execution rule:

- perform the cutover on one branch
- validate published-tool parity end to end
- only then delete vendored members from the branch that merges

Reason:

- the main unknown is released-tool parity, not the local ATM wrapper code
- this keeps rollback to one revert or one abandoned cutover branch rather than
  a second recovery sprint

## Execution Plan Once A Published Release Exists

### Step 1: Freeze the release candidate ATM will target

Deliverables:

- record the exact published version in the migration branch
- verify whether ATM must install:
  - crates from crates.io
  - GitHub release binaries
  - both
- verify Windows installation is first-class and non-Homebrew

Acceptance gate:

- one exact published version is named
- one supported local/CI installation path is chosen for all three CI
  platforms

### Step 2: Prove published crate and binary parity on a no-delete branch

Deliverables:

- add a temporary branch-local install path for the published tools
- keep vendored crates present while wrappers are retargeted and compared
- run side-by-side spot checks for:
  - `sc-boundary`
  - `sc-portability`
  - `unix-gating`
  - `runtime-waits`

Acceptance gate:

- published analyzers reproduce the required rule IDs
- published proc-macro crates compile `atm-core` unchanged or with explicitly
  approved attribute fixes

### Step 3: Retarget ATM wrappers and tests

Required code moves:

- change `.just/lint_sc_boundary.py` to call the published boundary analyzer
- change `.just/lint_sc_portability.py` to call the published portability
  analyzer
- change `.just/lint_unix_gating.py` to filter `PORT-004` and `PORT-005` from
  the published portability analyzer output
- change `.just/lint_runtime_waits.py` to filter `SCB-RUNTIME-001` and
  `SCB-RUNTIME-002` from the published runtime analyzer output
- update `.just/tests/test_lint_*.py` command assertions
- update `.just/tests/test_run_lint.py` only if target wiring changes

Acceptance gate:

- ATM lint names stay the same
- ATM report shape stays the same
- only backend commands change

### Step 4: Retarget compile-time dependencies

Required code moves:

- replace `crates/atm-core/Cargo.toml` path dependency on
  `sc-lint-attributes` with the published version
- update any internal version pins as needed
- verify `sc-lint-directives` resolves transitively or add it explicitly only
  if the published proc-macro surface still requires it

Acceptance gate:

- `cargo build --workspace` no longer depends on vendored `sc-lint-*` crates
- the current `#[sc_lint(...)]` usage in `crates/atm-core/src/observability.rs`
  compiles cleanly
- a targeted compile/behavior check covers the exact proc-macro usage at
  `crates/atm-core/src/observability.rs:209` and `:309`
  - if no focused test already exists, the cutover branch adds one
  - the minimum acceptable proof is a dedicated test or compile check that
    exercises `#[sc_lint(boundary.allow("cycle.recursive_value_container"))]`
    under the published proc-macro dependency, not only a whole-workspace build

### Step 5: Remove vendored workspace members

Required code moves:

- delete `crates/sc-lint-directives`, `crates/sc-lint-attributes`, and
  `crates/sc-lint-boundary` from the ATM workspace
- update `Cargo.lock`
- update any tests or helper scripts that assume those directories exist

Acceptance gate:

- no ATM workspace path dependency points at vendored `sc-lint-*`
- no lint wrapper still shells through `cargo run -p <vendored crate>`

### Step 6: Retarget CI and release-preflight install steps

Required code moves:

- add explicit installation of the published `sc-lint` tools in
  `.github/workflows/ci.yml`
- make the install path work on:
  - `ubuntu-latest`
  - `macos-latest`
  - `windows-latest`
- update `scripts/validate_release.py` assumptions if needed

Acceptance gate:

- `just lint` passes on all CI platforms using only the published tool install
- release preflight no longer assumes analyzer crates are in the ATM workspace

## Required Validation After Cutover

The first real migration branch must pass all of these:

- `cargo build --workspace`
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `python3 .just/run_lint.py sc-boundary`
- `python3 .just/run_lint.py sc-portability`
- `python3 .just/run_lint.py unix-gating`
- `python3 .just/run_lint.py runtime-waits`
- `python3 scripts/validate_release.py`
- `git diff --check`
- GitHub Actions `just-lint` on:
  - Ubuntu
  - macOS
  - Windows

Additional parity check:

- on the same ATM commit, compare vendored-tool findings vs published-tool
  findings before deleting the vendored crates
- any rule disappearance or unexpected finding-volume drop is a blocker until
  explained

## Rollback Plan

If the first published release misses any load-bearing ATM requirement:

- do not merge the cutover branch
- keep ATM on the vendored snapshot
- open a targeted follow-up against `sc-lint`
- retry only after the missing release surface is available

If regressions appear after the cutover branch exists but before merge:

- revert the published-tool retarget commit set on the branch
- keep the vendored crates intact until the follow-up release lands

No partial merge is acceptable where:

- proc-macro crates come from the published release, but analyzer parity is
  unproven
- or CI depends on ad hoc local checkouts of `../sc-lint`

## Open Questions For The sc-lint Team

1. Will the next published release include all of these as supported consumer
   surfaces:
   - `sc-lint-attributes`
   - `sc-lint-directives`
   - `sc-lint-boundary`
   - `sc-lint-portability`
   - `sc-lint-runtime`
2. What is the supported all-platform installation path for CI, especially on
   Windows:
   - `cargo install`
   - `cargo binstall`
   - release tarballs
3. Is the dedicated analyzer JSON output considered stable enough for consumer
   wrappers, or should ATM plan to consume only the top-level `sc-lint --json`
   contract?
4. Does the published portability surface expose an equivalent for the current
   `unix_path_prefixes` config, or is that setting intentionally removed?
5. Are the current rule IDs guaranteed to stay stable for:
   - `PORT-004`
   - `PORT-005`
   - `SCB-RUNTIME-001`
   - `SCB-RUNTIME-002`
6. Does the published proc-macro line preserve the current directive grammar
   used by ATM without requiring attribute-source edits?

## Exit Criteria For This Plan

This planning line is complete when:

- the current ATM `sc-lint` usage is fully inventoried
- the vendored-snapshot vs published-release gap is explicit
- the migration sequence is concrete enough to execute once a release exists
- the open questions above are raised before ATM commits to the cutover
