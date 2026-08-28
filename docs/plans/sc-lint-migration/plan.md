---
title: sc-lint Published Release Migration Inventory And Gap Analysis
plan_type: support_inventory
status: complete
branch: plan/sc-lint-published-migration
worktree: ../atm-core-worktrees/plan/sc-lint-published-migration
---

# sc-lint Published Release Migration Inventory And Gap Analysis

This document is retained as a supporting inventory and gap-analysis artifact.

## Phase AD Status Note

This branch originally attached a sc-lint execution proposal that stopped at
`sprint-AD1.md` through `sprint-AD9.md`.

That proposal never executed.

The `AD.*` namespace was later consumed by unrelated accepted ATM work:
[`../../project-plan.md`](../../project-plan.md) now records `Phase AD Caller
Identity And Post-Send Runtime Simplification [COMPLETE]`, and
[`../phase-AD/readiness.md`](../phase-AD/readiness.md) records closeout
through `AD.35` for that unrelated caller-identity / post-send correction
line.

Implications for this sc-lint migration plan:

- no sc-lint execution has occurred under any `AD.*` sprint
- the earlier sc-lint `AD.1` through `AD.9` proposal is a historical,
  unexecuted namespace snapshot only
- any future sc-lint execution line needs a new phase identifier rather than
  reuse of `AD.*`

This supporting doc remains useful because it captures:

- the original vendored-vs-published inventory
- the initial gap analysis
- the early no-change verification for boundary TOMLs and release manifests
- the Python lint ownership review and duplicate-surface classification

## Goal

Remove `atm-core`'s vendored `sc-lint` crates and cut over to the next
published `sc-lint` release without weakening any current lint, CI, or release
gates, while converging onto native released `sc-lint` surfaces instead of
retaining unnecessary ATM-local wrappers or suppressing findings.

Plain-language target end state:

- vendored `sc-lint` crates are gone from the ATM workspace
- most repo-local Python lint wrapper code is gone
- ATM consumes released `sc-lint` tools directly for analyzer execution, JSON
  output, and rule selection wherever the product already supports that
- only narrowly justified ATM-owned policy code remains, and only where the
  released `sc-lint` product does not and should not own that behavior

This is planning only. No cutover begins until a published `sc-lint` version
exists that covers the current `atm-core` dependency and analyzer surface.

The inventory/gap-analysis task was the originally scoped deliverable for this
branch. The sc-lint `AD.1` through `AD.9` package was added later as one
proposed execution line for this migration. It was never an execution
authorization by itself and it cannot be reused now that `AD.*` has been
permanently consumed by unrelated accepted ATM work. Any future sc-lint
execution branch still requires explicit human sign-off and a new phase
identifier before implementation starts.

## Authoritative Sprint Package

The execution scope for this migration is authoritative in the sprint docs
listed below, not in downstream prompts or inferences.

This inventory doc remains the supporting baseline and gap-analysis artifact.
If execution starts, QA should review the sprint docs directly.

- `docs/plans/sc-lint-migration/sprint-01.md`
  - freeze the target published release and build the initial gap register
- `docs/plans/sc-lint-migration/sprint-02.md`
  - establish published-tool parity baselines and classify every mismatch
- `docs/plans/sc-lint-migration/sprint-03.md`
  - migrate `sc-boundary` to direct published-tool usage and delete the thin
    wrapper
- `docs/plans/sc-lint-migration/sprint-04.md`
  - migrate `sc-portability` to direct published-tool usage and delete the thin
    wrapper
- `docs/plans/sc-lint-migration/sprint-05.md`
  - resolve `unix-gating` against the published portability surface
- `docs/plans/sc-lint-migration/sprint-06.md`
  - resolve `runtime-waits` against the published runtime surface
- `docs/plans/sc-lint-migration/sprint-07.md`
  - reduce or delete `run_lint.py` and preserve the user-facing lint surface
- `docs/plans/sc-lint-migration/sprint-08.md`
  - remove duplicated ATM-local dependency-policy and boundary-governance logic
    where released `sc-lint` already owns it
- `docs/plans/sc-lint-migration/sprint-09.md`
  - retarget any compile-time `sc-lint-*` dependencies that legitimately remain
- `docs/plans/sc-lint-migration/sprint-10.md`
  - delete the vendored workspace members
- `docs/plans/sc-lint-migration/sprint-11.md`
  - retarget CI and release-preflight to the published install path
- `docs/plans/sc-lint-migration/sprint-12.md`
  - enable all adopted released `sc-lint` features and capture the resulting
    ATM delta without suppressing warnings
- `docs/plans/sc-lint-migration/sprint-13.md`
  - resolve all non-architectural ATM findings exposed by the enabled
    `sc-lint` feature set
- `docs/plans/sc-lint-migration/sprint-99.md`
  - final implementation review, sc-lint gap reports, residual adapter audit,
    and product-improvement follow-ups

`sprint-99.md` is intentionally reserved as the terminal review sprint and
must remain numerically last even if additional execution sprints are inserted
later.

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
| root `Cargo.toml` workspace members | vendors `crates/sc-lint-directives`, `crates/sc-lint-boundary` (inventory updated 2026-08-27: `crates/sc-lint-attributes` was vendored by Phase AU, then removed by PR #1068 in favor of the published crate — see the recorded exception below) | the analyzers compile as part of the ATM workspace today | the two remaining vendored members must be removed from the workspace during cutover |
| `crates/atm-core/Cargo.toml` | depends on published `sc-lint-attributes = "0.5"` (since PR #1068; the Phase AU vendored path dependency preceded it) | the published proc-macro dependency is intentional and release-preflight-clean | already on the published surface; no further cutover action for this crate's `sc-lint` dependency |
| `crates/atm-core/src/observability.rs` | two `#[sc_lint(boundary.allow("cycle.recursive_value_container"))]` call sites (introduced by Phase AU) | live proc-macro call sites compiled against the published `0.5` crate and honored by the vendored `sc-boundary` analyzer | analyzer cutover must preserve these allows' effect |

### B. Local lint wrappers and command entry points

| Current ATM asset | Current command shape | Current role | Migration impact |
| --- | --- | --- | --- |
| `.just/lint_sc_boundary.py` | `cargo run -q -p sc-lint-boundary -- analyze --root <repo> --format json` | manual full boundary analyzer wrapper | must stop calling the vendored workspace binary |
| `.just/lint_sc_portability.py` | `cargo run -q -p sc-lint-boundary -- analyze --root <repo> --rule portability --format json` | manual portability analyzer wrapper | must retarget to published portability surface |
| `.just/lint_unix_gating.py` | same portability command, then filter `PORT-004` and `PORT-005` | default-house lint wrapper for Unix gating | default disposition is deletion; retention is allowed only if the gap register proves one named published portability gap and records a deletion trigger |
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
| `.just/run_lint.py` | local lint orchestration and house target naming | reduce or replace | preferred end state is direct `Justfile`/native `sc-lint` wiring; keep only as a temporary compatibility shim if ATM cannot preserve the user-facing lint surface without it during transition |
| `.just/lint_sc_boundary.py` | thin full-boundary analyzer wrapper | delete | should be replaced by direct invocation of published `sc-lint-boundary` rather than another ATM-local wrapper |
| `.just/lint_sc_portability.py` | thin portability analyzer wrapper | delete | should be replaced by direct invocation of published `sc-lint-portability` rather than another ATM-local wrapper |
| `.just/lint_unix_gating.py` | repo-specific portability subset wrapper | reduce or replace | keep only if published `sc-lint-portability` cannot express the subset directly; otherwise delete it too |
| `.just/lint_runtime_waits.py` | repo-specific runtime-waits subset wrapper | reduce or replace | keep only if published `sc-lint-runtime` cannot express the subset directly; otherwise delete it too |
| `.just/lint_boundaries.py` | ATM boundary-TOML schema, allowlist, review-gate, and dependency-policy checks | keep partially | ATM-specific governance may remain, but dependency-policy overlap with released `sc-lint` `D.1` must be deleted wherever the published surface is equivalent or stronger |

Published `sc-lint` coverage already exists for the main Rust-owned lint
families that ATM still reaches through Python today:

- `sc-lint-boundary`
  - shared owner for boundary-cycle / boundary-visibility analysis
- `sc-lint-portability`
  - shared owner for portability analysis including `PORT-004` and `PORT-005`
- `sc-lint-runtime`
  - shared owner for runtime/concurrency analysis including
    `SCB-RUNTIME-001` and `SCB-RUNTIME-002`

Planning implication:

- any ATM Python file that only invokes one of those Rust analyzers, parses its
  JSON, and reformats/filter-selects findings is presumed duplicate until
  proven otherwise
- the current file-by-file default call is:
  - delete `.just/lint_sc_boundary.py`
  - delete `.just/lint_sc_portability.py`
  - delete `.just/lint_unix_gating.py` unless one named portability gap blocks
    direct native usage
  - delete `.just/lint_runtime_waits.py` unless one named runtime gap blocks
    direct native usage
  - delete `.just/run_lint.py` unless one named consumer-surface gap requires a
    temporary compatibility shim
  - keep only the ATM-specific portions of `.just/lint_boundaries.py`, deleting
    any logic the released `sc-lint` product now owns
- future execution must explicitly inventory every `.just/lint_*.py` file into
  one of these buckets:
  - delete because published `sc-lint` already covers it directly
  - shrink to a minimal ATM adapter because only the consumer-repo surface is
    ATM-specific
  - keep because it is not a `sc-lint` concern at all

### E. Duplicate-surface classification

Duplicate implementation expected to be removed during execution:

- vendored crates:
  - `crates/sc-lint-directives`
  - `crates/sc-lint-attributes`
  - `crates/sc-lint-boundary`
- thin analyzer wrappers whose only job is shelling out to vendored or
  published `sc-lint` binaries:
  - `.just/lint_sc_boundary.py`
  - `.just/lint_sc_portability.py`
- any analyzer invocation path that still relies on:
  - `cargo run -q -p sc-lint-boundary`
  - a workspace-built vendored `sc-lint` binary
- any repo-local Python subset wrapper that the released `sc-lint` surface can
  replace with a direct rule/selector/config invocation
- any ATM-local dependency-policy enforcement in `.just/lint_boundaries.py`
  that the released `sc-lint` `D.1` boundary scan proves equivalent or stronger

ATM-owned surface expected to remain after migration:

- repo-local lint names in the `Justfile` and release/preflight surfaces
- only the ATM-specific boundary schema / allowlist / review-gate checks in
  `.just/lint_boundaries.py`; duplicated dependency-policy or analyzer-owned
  enforcement must not remain there
- only the minimum residual adapter code required during transition, with each
  survivor documented by a concrete justification, an upstream gap reference,
  and a deletion trigger back to native `sc-lint` usage
- tests that protect ATM-owned behavior still justified after the Python
  deletion review

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
2. deletion of duplicate Python adapter code wherever released Rust analyzers
   already provide the same capability
3. wrapper/CI retargeting from one integrated analyzer binary to the released
   analyzer family surface

## Gap Analysis

| Current ATM requirement | Current ATM source of truth | Expected published `sc-lint` coverage | Risk / note |
| --- | --- | --- | --- |
| compile-valid `#[sc_lint(...)]` proc-macro support for `boundary.allow`, `boundary.internal_only`, `boundary.forbid_external_impls` when ATM intentionally reintroduces those attributes | no live ATM compile-time dependency today | expected yes if a future execution branch chooses to restore the proc-macro surface | low risk, but currently unneeded because the accepted ATM line has no live `#[sc_lint(...)]` usage to retarget |
| boundary-cycle and boundary-visibility findings from `sc-lint-boundary analyze --format json` | vendored `sc-lint-boundary` | expected yes | moderate risk: ATM must verify JSON field compatibility, not just rule existence |
| graph export via `sc-lint-boundary export-graph` | vendored `sc-lint-boundary` | expected yes | low risk; no active ATM CI call site uses it today |
| portability findings on demand (`sc-portability`) | vendored `sc-lint-boundary --rule portability` | expected yes through `sc-lint-portability` | moderate risk because the analyzer owner and command path changed |
| default lint subset for `PORT-004` / `PORT-005` (`unix-gating`) | ATM wrapper filters portability findings | not a product feature by itself | preferred end state is direct published-tool selection with no ATM-local subset wrapper; if impossible, the residual wrapper must be explicitly justified and marked for later deletion |
| default lint subset for `SCB-RUNTIME-001` / `SCB-RUNTIME-002` (`runtime-waits`) | ATM wrapper filters boundary findings | semantic coverage expected through `sc-lint-runtime`, but rule-id continuity is unproven | preferred end state is direct published-tool selection with no ATM-local subset wrapper; if impossible, the residual wrapper must be explicitly justified and marked for later deletion |
| current `just lint` names and report formatting | `Justfile` plus any surviving minimal adapter code | not expected upstream | ATM should preserve the user-facing lint surface while deleting redundant Python; any surviving adapter must have a concrete reason to exist |
| dependency-policy enforcement from `dependencies.allowed_*` and `forbidden_edges` | partly duplicated in `.just/lint_boundaries.py` today | expected yes once released `sc-lint` Phase `D.1` lands | migration is not complete until ownership moves to released `sc-lint` or ATM explicitly proves why a residual check stays local |
| all-platform CI bring-up with no local vendored analyzer crates | workspace build today | unknown until release artifacts are published | high risk until install story is confirmed for Windows, macOS, and Linux |
| portability config knob `unix_path_prefixes` | vendored `sc-lint-boundary/config/defaults.toml` | uncertain | must confirm whether the published portability crate exposes an equivalent config surface or bakes the defaults in |
| release-preflight lint behavior | `scripts/validate_release.py` | not expected upstream | ATM-owned release gate must be retargeted |

The unresolved `unix_path_prefixes` portability-config gap is a hard planning
input for the later `unix-gating` cutover. No accepted sc-lint execution sprint
currently owns that closure, because the earlier `AD.4` reference belongs to
an unexecuted sc-lint proposal whose namespace was later consumed by unrelated
work. Any future migration execution branch must still prove that the
published `sc-lint-portability` surface either provides an equivalent knob or
that ATM carries the behavior forward in a documented wrapper-owned override.

General execution policy for any additional gap:

- identify it during planning if the evidence is already available
- otherwise identify it on the integration branch at the first failing parity
  check
- queue the missing capability with the sc-lint team immediately
- keep only the narrowest possible ATM-local workaround until the upstream
  capability exists

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

### Decision 1: Default to deleting ATM-local Python wrappers

This is the actual migration goal.

Reason:

- the purpose of this migration is not only to swap vendored crates for
  published ones, but to retire duplicated ATM-local implementation where the
  released `sc-lint` surface already covers the need
- thin Python wrappers that only shell out to analyzers are maintenance
  overhead, not a load-bearing ATM product requirement
- the released `sc-lint` line already owns the Rust analyzer families behind
  `sc-boundary`, `sc-portability`, and `runtime-waits`, so preserving Python
  by default would keep duplicate implementation on the ATM side without
  architectural justification
- `sc-lint` is the long-term product line; ATM should consume its released
  tools as designed rather than retaining repo-local wrappers as a parallel
  product surface
- the user-facing contract ATM must preserve is the lint entrypoint and release
  gate behavior, not the existence of repo-local Python wrappers

Execution rule:

- start from deletion as the default for `.just/lint_*.py`
- re-add or retain only the smallest possible adapter code when the published
  `sc-lint` surface cannot express an ATM-specific contract directly
- long-term convergence target is zero unnecessary wrappers: once released
  `sc-lint` covers the needed behavior, ATM must call the tool directly
- every residual Python file must name:
  - the ATM-specific behavior it still owns
  - why the published tool cannot replace it yet
  - the condition that allows later deletion

### Decision 1a: Gaps in released `sc-lint` must be surfaced and queued

Execution rule:

- if the current planning line can already prove a released-`sc-lint`
  capability gap, record it now and queue follow-up work with the sc-lint team
  before execution starts
- if a gap is only discovered during the real migration branch, stop deletion
  for that exact surface, document the gap immediately, and queue the follow-up
  with the sc-lint team before any partial workaround merges
- temporary ATM-local Python or adapter code is acceptable only for a proven
  missing released capability, not as a convenience default
- the expected steady state is that released `sc-lint` is as good as or better
  than the historical ATM-local implementation for every adopted lint function

Acceptance rule:

- every surviving workaround must link to one named gap
- every named gap must state whether it is:
  - queued before migration started
  - discovered during integration and queued immediately
- no workaround may survive without both:
  - an explicit sc-lint follow-up owner
  - a deletion trigger tied to the missing capability landing upstream

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

### Decision 3: Do not delete the vendored path or residual Python until parity is proven

Execution rule:

- perform the cutover on one branch
- validate published-tool parity end to end
- only then delete vendored members from the branch that merges

Reason:

- the main unknown is released-tool parity, not the local ATM wrapper code
- this keeps rollback to one revert or one abandoned cutover branch rather than
  a second recovery sprint
- Python deletion should follow the same rule: delete only after direct
  published-tool usage or a smaller justified adapter has been proven end to
  end

## Execution Plan Once A Published Release Exists

The authoritative deliverables, acceptance criteria, delete lists, and
validation gates now live in the sprint docs named in the
`Authoritative Sprint Package` section above.

Execution summary only:

1. `sprint-01.md`
   - freeze the target release and initial gap register
2. `sprint-02.md`
   - prove parity and classify every mismatch
3. `sprint-03.md` through `sprint-07.md`
   - delete or reduce wrapper surfaces in favor of native released `sc-lint`
4. `sprint-08.md` through `sprint-11.md`
   - remove duplicated governance where appropriate, retarget compile-time
     dependencies, delete vendored crates, and move CI/preflight to the
     published install path
5. `sprint-12.md` and `sprint-13.md`
   - enable adopted released features and clean all non-architectural ATM
     findings instead of suppressing them
6. `sprint-99.md`
   - perform the terminal review, upstream gap report, residual adapter audit,
     and sc-lint product-improvement report

If any future execution sprint needs to change a deliverable or gate, that
change must land in the sprint doc itself rather than by extending this summary
section.

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

Recorded exception (2026-08-27): PR #1068 moved `sc-lint-attributes` alone to
the published `0.5` crate and deleted the vendored copy, under Rand's direct
authorization, to clear the v1.4.4 release-preflight publish-dependency
blocker. Proc-macro parity was proven in that PR (compile, lint gate with the
allows honored, tests, clean `just validate`); the analyzers remain vendored
and fully gated. See the Partial-Scope Exception note in
[sprint-10.md](./sprint-10.md).

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
- the documented target end state is unambiguous:
  - vendored `sc-lint` crates are obsoleted
  - most Python lint wrapper code is obsoleted
  - released `sc-lint` tools are consumed directly wherever they provide
    equivalent or better native functionality
  - only explicitly justified ATM-specific policy layers remain
- the open questions above are raised before ATM commits to the cutover
