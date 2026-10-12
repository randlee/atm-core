# Phase bg: migrate to published sc-lint

Rendered from the beads plan (`bd show`). The beads are authoritative; this file is for review.

---

## atm-phase-bg: phase-bg: migrate atm-core lint to published sc-lint 0.7.0

## Goal
atm-core's lint runs on the published sc-lint 0.7.0 crates, wheel and adoption kit, and every local lint tool sc-lint duplicates is deleted after its output is compared. Intent: `docs/plans/phase-bg/intent.md`; anchors ANC-001..021: `docs/plans/phase-bg/anchors.json` (branch plan/phase-bg, PR #1762, until it merges). Replaces the stalled phase-be (atm-phase-be), which waited on 0.6.x.

## Sprints (one stack, one package per step, ANC-019)
| Layer | Sprint | Package | Depends on |
| --- | --- | --- | --- |
| 1 | bg-1 | adoption kit (just setup/lint/upgrade), xwin clippy | plan review |
| 2 | bg-2 | sc-lint-boundary (sc-boundary) | bg-1 |
| 3 | bg-3 | sc-lint-portability (sc-portability, unix-gating) | bg-2 |
| 4 | bg-4 | sc-lint-runtime (runtime-waits); vendored crates deleted | bg-3 |
| 5 | bg-5 | sc_lint Python wheel tools | bg-4 |
| 6 | bg-6 | runner cutover: parallel steps, `just lint ci` platform scope, `just test` manifest, speed | bg-5, gate atm-phase-bg-gate-sclint-runner |

## Phase rules
- atm-core-specific config goes in the kit's consumer input (`.sc-lint/install.json`) and recipes outside the kit block; kit-managed files are never edited, and a missing extension point is requested on randlee/sc-lint (ANC-002).
- Every sc-lint gap or error found in any sprint is filed on randlee/sc-lint and sent to team-lead@sc-lint when found (ANC-020).
- Where atm-core needs boilerplate to use sc-lint, the sprint that writes it files an issue on randlee/sc-lint for a template or consumer-facing code that removes it, or records why not (ANC-018).

### Design

## Serial order
The sprints form one chain bg-1→…→bg-6 (bg-6 also behind the runner gate) because Rand ruled it: "migrate and replace packages one at a time, noting any gaps or errors" (2026-10-09; intent.md "one package at a time"; ANC-019). Each package sprint also rewrites the same `profiles` list in `.sc-lint/install.json` and the same `run_lint.py` target table, so parallel siblings would conflict on those files. With one sprint active at a time, the shared `crates/` fence (fixes for findings the analyzer reports, location unknown until it runs) has one owner at a time.

## Decisions
- Kit-managed files are never hand-edited (ANC-002). atm-core specifics live in `.sc-lint/install.json` (its `profiles`, `test` and `analyzers`) and in recipes outside the managed Justfile block. `sc-lint.toml` is rendered from `.sc-lint/install.json` by `python3 plugins/sc-lint/install.py .` and committed as rendered, never hand-edited. Analyzer keys are the v0.7.0 schema only: boundary and runtime take enabled, reason, exclude_rules, fail_rules, exclude_paths; portability also takes config_home_env, unix_path_prefixes, targets.
- atm-core-only checks (the 20 steps with no sc-lint equivalent: hermes-*, site-*, daemon-*, env-var-boundary, nudge-taxonomy, arch-gates, ...) stay atm-core steps (ANC-021); none is proposed to sc-lint.
- sc-publish files (ADR-050) are never edited; kit 0.7.0 leaves them alone because `ci.enabled = false` excludes `.github/` and the installer refuses clashes (sc-lint#193 stays open).

- Until bg-6, the CI just-lint job keeps running `.just/run_lint.py all` (parallel, reports every failure); the kit steps are selectable through `just lint <step>`. bg-6 switches it. The pre-push hook (`.githooks/pre-push`: fmt, clippy, ttl-triage, validate-findings, site) calls those commands directly, not `run_lint.py`, and is not changed in this phase.
- Every sprint that deletes a legacy script also removes its `run_lint.py` target, Justfile recipe and `just help` line, and leaves no importer of the deleted module.
- `just lint` installs and runs sc-lint through the kit (`just setup` → `.sc-lint/bootstrap`, which owns the binary and `.sc-lint/venv`). The sc-publish action `.github/actions/setup-sc-lint` is used only by sc-publish workflows and is not touched.
- Cross-OS proof: CI lints macOS and Windows only on PRs to main (ci.yml, Rand 2026-09-26). Layer PRs are proven on Linux CI; Windows and macOS CI are proven on the phase release PR (phase AC).

## Gate
- `atm-phase-bg-gate-sclint-runner` (blocks bg-6): a published sc-lint release with #195 (parallel/keep-going steps), #196 (consumer test manifest) and #199 (platform scope selector). #197 (per-step target) and #198 (runner python) are not in the gate; bg-6 keeps the recipes that cover them, each marked `delete when`.

### Acceptance criteria

## Phase acceptance
- [ ] `just lint` at the bg-6 head runs only published sc-lint crates and atm-core-only steps; `crates/sc-lint-boundary` and `crates/sc-lint-directives` are gone.
- [ ] Every duplicate legacy lint tool is deleted.
- [ ] `just lint` at the bg-6 head is faster than at the phase base commit, measured in bg-6 (BG6-4).
- [ ] Windows and macOS CI pass `just lint` on the phase release PR.
- [ ] Every suppression added in the phase has an open finding bead.

---

## atm-bg-1: bg-1: Kit install

## Goal
Install the sc-lint 0.7.0 adoption kit verbatim so atm-core has the shared `just setup` / `just lint` / `just upgrade` contract (ANC-001), with every check `just lint` runs today still running under the same name, plus Windows-target clippy on macOS and Linux through xwin (ANC-005).

## Deliverables
1. BG1-1 Kit apply: `python3 <kit-0.7.0>/install.py --input <input> .` writes `.sc-lint/install.json`, `sc-lint.toml`, `.sc-lint/{justfile,bootstrap,bootstrap.ps1,installed.json}`, `plugins/sc-lint/**`, the adopter agent and skill, and the managed Justfile import block. Kit files are committed unedited; `python3 plugins/sc-lint/install.py --dry-run .` exits 0 with no output.
2. BG1-2 Steps: `.sc-lint/install.json` `profiles` has one step per current `run_lint.py all` target, same name, argv `["just", "_py", ".just/run_lint.py", "<target>"]`; `test` keeps today's `just test` (the kit leaves an existing `test` recipe alone, sc-lint#194).
3. BG1-3 Justfile: remove the recipes that collide with the kit justfile (`lint target=`, `default` if it collides); add `_py *args` (runs the pinned `.bootstrap-venv` interpreter for the host OS); every other recipe unchanged. Bare `just` still prints help.
4. BG1-4 xwin: step `clippy-windows` runs the published `sc-lint clippy xwin` (and `clippy` runs `sc-lint clippy native`); the only wrapper is the PYO3_PYTHON environment the pyo3 crates (atm-query-python, atm-graft-python) need, marked `delete when randlee/sc-lint#198 ships`. On Windows `clippy-windows` exits 0 saying native clippy covers it. `cargo-xwin` pinned in `tools/bootstrap.toml`; `tools/bootstrap.py` adds the msvc target on non-Windows hosts; bootstrap tests and doc updated.
5. BG1-5 CI: `.github/workflows/ci.yml` lint and test jobs run `just setup` after bootstrap and cache the sc-lint binary and the xwin SDK; the just-lint job, which calls `just lint` today, calls `just _py .just/run_lint.py all` until bg-6; there is still one lint workflow (no kit `sc-lint.yml`, `ci.enabled = false`).

## Does Not Close
- Retargeting steps to published analyzers and deleting legacy tools (bg-2..bg-5); runner cutover and speed (bg-6).

### Design

## Kit model (sc-lint v0.7.0, packages/sc-lint-adoption)
- `install.py` persists its validated input at `.sc-lint/install.json`; `--input` bootstraps it. `profiles` and `test` argv are rendered verbatim into `sc-lint.toml` `[[tool.sc-lint.lint]]` / `[[tool.sc-lint.test]]`.
- `.sc-lint/justfile`: `setup`, `lint *profile`, `upgrade` via `.sc-lint/bootstrap` (`.ps1` on Windows). `test.just` is added only when the consumer has no `test` recipe; atm-core has one.
- Consumer argv runs with no shell, so every atm step goes through `just _py ...`, which picks the bootstrap venv Python on each OS.
- Kit steps run serially and stop at the first failure until sc-lint#195 ships (bg-6); CI keeps `run_lint.py all` meanwhile.

## Upstream
- Gaps found here, including consumer boilerplate a kit template could remove (sc-lint#348 covers the installer renderer), are filed on randlee/sc-lint as general-purpose capabilities and sent to team-lead@sc-lint (ANC-018, ANC-020).

### Acceptance criteria

## Acceptance Criteria
- [ ] `python3 plugins/sc-lint/install.py --dry-run .` exits 0, no output; no kit-managed file differs from the 0.7.0 kit render.
- [ ] `just lint` runs every step `run_lint.py all` ran on develop, by name, plus `clippy-windows`; the ADR-008 gates (fixed-sleep, runtime-waits, daemon-singleton, same-host-portability) are named steps; Linux CI passes.
- [ ] Each step's argv in `.sc-lint/install.json` is `["just", "_py", ".just/run_lint.py", "<target>"]` or a published sc-lint command; `just _py --version` prints the `.bootstrap-venv` Python; bare `just` prints help.
- [ ] `just lint clippy-windows` runs `sc-lint clippy xwin` with `-D warnings` on macOS/Linux and exits 0 on Windows; `cargo-xwin` is pinned in `tools/bootstrap.toml` and bootstrap adds the msvc target (bootstrap tests cover both).
- [ ] `.github/workflows/ci.yml` runs `just setup`, caches the sc-lint binary and xwin SDK, and has one lint workflow; Windows and macOS legs are proven on the phase release PR (phase AC).

## Validation
- `just setup && just lint`
- `just test`
- `python3 plugins/sc-lint/install.py --dry-run .`

---

## atm-bg-2: bg-2: Published sc-boundary

## Goal
The `sc-boundary` step runs the published `sc-lint lint sc-boundary` configured by `analyzers.boundary` in `.sc-lint/install.json` (ANC-009, ANC-010), and the vendored path plus the parts of `.just/lint_boundaries.py` it duplicates are deleted after comparison (ANC-011, ANC-012).

## Deliverables
1. BG2-1 Inventory: the boundary TOMLs load under the published loader. Keys the loader rejects (`[enforcement] no_in_repo_implementation`, `io_forbidden_source_modules`) move under `[extensions.atm]` (sc-lint#203); `boundaries/planning.toml` with `[planning].current_sprint` is added, because the published boundary loader (v0.7.0) refuses to run without it; it is deleted if a later sc-lint release drops that requirement.
2. BG2-2 Retarget: step `sc-boundary` = `sc-lint lint sc-boundary`; `analyzers.boundary` set in `.sc-lint/install.json` and `sc-lint.toml` re-rendered by install.py.
3. BG2-3 Compare and delete: vendored vs published finding sets at one commit in the PR body; `.just/lint_sc_boundary.py` and its tests deleted. In `.just/lint_boundaries.py`, every rule the published Rust analyzer or the wheel's `sc_lint.lint_boundaries` now enforces (inventory schema, workspace dependency edges) is deleted after the same comparison against both; `.just/view_boundaries.py` and its test follow the moved fields; atm-only rules (SCB-CONFIG/RETAINED/WORKSPACE/SINGLETON/OBSERVABILITY, third-party manifest allowlists until sc-lint#136) stay. ADR-001 (Enforcement Layer 1), ADR-016, ADR-020, ADR-054 and `docs/architecture/boundary/general-guidelines.md` name the tool that now enforces each rule they cite.
4. BG2-4 Findings: every new published-analyzer finding fixed in code or carried by a finding bead (rules in design).

## Does Not Close
- Deleting `crates/sc-lint-boundary` (still used by portability and runtime steps until bg-4).

### Design

## Rules every package step follows (ANC-011, 013, 014, 016, 019, 020, 021)
- Compare before delete: at one commit, run the legacy tool and its sc-lint replacement, and put both commands and the finding-set diff in the PR body. A finding the legacy tool reports that sc-lint does not is a gap: file it on randlee/sc-lint (general-purpose wording, no atm-core specifics) and send the issue number to team-lead@sc-lint the same day; keep the legacy check only for that finding, with `# delete when randlee/sc-lint#<n> ships`.
- New sc-lint findings are fixed in code. A length finding gets a refactor that makes the code clearer; moving lines to land just under a limit is rejected in review.
- A suppression attribute or allowlist entry is allowed only with a stated reason; each one added to get CI green gets a finding bead under the phase root in the same PR.
- Delete the legacy tool, its tests and its run_lint.py target in the same sprint, once the comparison shows no unfiled gap.

### Acceptance criteria

## Acceptance Criteria
- [ ] `sc-lint lint sc-boundary --config sc-lint.toml` exits 0 at the head; its config is `analyzers.boundary` in `.sc-lint/install.json` plus the `boundaries/` inventory.
- [ ] Every boundary TOML loads under the published loader with the moved keys under `[extensions.atm]`; `boundaries/planning.toml` exists; `just view boundaries` still renders (its test passes).
- [ ] `.just/lint_sc_boundary.py`, its `run_lint.py` target and tests are deleted; `.just/lint_boundaries.py` no longer implements a rule the published analyzer enforces; the ADRs above name the enforcing tool.

## Validation
- `just lint`
- `just test`
- `cargo build --workspace`

---

## atm-bg-3: bg-3: Published sc-portability

## Goal
The `sc-portability` and `unix-gating` steps run the published `sc-lint lint sc-portability` configured by `analyzers.portability` in `.sc-lint/install.json` (ANC-009, ANC-010); the vendored wrappers are deleted after comparison; the findings PORT-006..010 newly report are fixed in code (ANC-013).

## Deliverables
1. BG3-1 Config: `[portability]` in `.just/lint-config.toml` moves to `analyzers.portability` in `.sc-lint/install.json` (`config_home_env = "ATM_CONFIG_HOME"`, plus `unix_path_prefixes` and `targets` where atm needs them); `sc-lint.toml` re-rendered by install.py.
2. BG3-2 Retarget: `sc-portability` = `sc-lint lint sc-portability`, which covers PORT-004/005; the `unix-gating` step is removed (a PORT-004/005 finding the legacy step reports and the published one does not is a gap, handled by the rules). The embedded portability analyzer in `crates/sc-lint-boundary` has no caller after this sprint and is deleted with the crate in bg-4.
3. BG3-3 Compare and delete: finding-set diff in the PR body; `.just/lint_sc_portability.py`, `.just/lint_unix_gating.py` and their tests deleted.
4. BG3-4 Findings: every PORT-001..010 finding fixed in code; a reviewed exception the analyzer cannot express (sc-lint#192 class) carries a reason and a finding bead.

## Does Not Close
- `same-host-portability` (atm-only, stays); deleting the embedded analyzer crate (bg-4).

### Design

## Rules every package step follows (ANC-011, 013, 014, 016, 019, 020, 021)
- Compare before delete: at one commit, run the legacy tool and its sc-lint replacement, and put both commands and the finding-set diff in the PR body. A finding the legacy tool reports that sc-lint does not is a gap: file it on randlee/sc-lint (general-purpose wording, no atm-core specifics) and send the issue number to team-lead@sc-lint the same day; keep the legacy check only for that finding, with `# delete when randlee/sc-lint#<n> ships`.
- New sc-lint findings are fixed in code. A length finding gets a refactor that makes the code clearer; moving lines to land just under a limit is rejected in review.
- A suppression attribute or allowlist entry is allowed only with a stated reason; each one added to get CI green gets a finding bead under the phase root in the same PR.
- Delete the legacy tool, its tests and its run_lint.py target in the same sprint, once the comparison shows no unfiled gap.

### Acceptance criteria

## Acceptance Criteria
- [ ] `sc-lint lint sc-portability --config sc-lint.toml` exits 0 at the head; `[portability]` is gone from `.just/lint-config.toml`.
- [ ] Both wrappers, their tests, their `run_lint.py` targets and the `unix-gating` step are deleted; `analyzers.portability` is set in `.sc-lint/install.json`.

## Validation
- `just lint`
- `just test`

---

## atm-bg-4: bg-4: Published sc-runtime; vendored crates removed

## Goal
The `runtime-waits` step runs the published `sc-lint lint sc-runtime` configured by `analyzers.runtime` in `.sc-lint/install.json` (ANC-009, ANC-010), and, with no step left on them, the vendored `crates/sc-lint-boundary` and `crates/sc-lint-directives` are deleted (ANC-012).

## Deliverables
1. BG4-1 Retarget: `runtime-waits` = `sc-lint lint sc-runtime` as its own step (it is not in the built-in ci profile, sc-lint#344).
2. BG4-2 Compare and delete: SCB-RUNTIME-001/002 vendored vs published finding-set diff in the PR body; `.just/lint_runtime_waits.py` and its tests deleted.
3. BG4-3 Vendored removal: `crates/sc-lint-boundary` and `crates/sc-lint-directives` removed from the workspace, `Cargo.toml` and `Cargo.lock`; `.just/tests/test_prerelease_archive_workflow.py` expects the registry `sc-lint-directives` entries the lockfile holds (0.4.0 and 0.5.0 at the cut commit), none path-sourced.
4. BG4-4 Docs: REQ-P-LINT-POSTMORTEM-001 (`docs/requirements.md`) and `docs/architecture.md` describe the published sc-lint crates instead of the embedded `crates/sc-lint-*` surface; `docs/architecture/boundary/general-guidelines.md` stops citing `crates/sc-lint-boundary/config/defaults.toml`.
5. BG4-5 Findings: new runtime findings fixed in code or carried by a finding bead.

### Design

## Rules every package step follows (ANC-011, 013, 014, 016, 019, 020, 021)
- Compare before delete: at one commit, run the legacy tool and its sc-lint replacement, and put both commands and the finding-set diff in the PR body. A finding the legacy tool reports that sc-lint does not is a gap: file it on randlee/sc-lint (general-purpose wording, no atm-core specifics) and send the issue number to team-lead@sc-lint the same day; keep the legacy check only for that finding, with `# delete when randlee/sc-lint#<n> ships`.
- New sc-lint findings are fixed in code. A length finding gets a refactor that makes the code clearer; moving lines to land just under a limit is rejected in review.
- A suppression attribute or allowlist entry is allowed only with a stated reason; each one added to get CI green gets a finding bead under the phase root in the same PR.
- Delete the legacy tool, its tests and its run_lint.py target in the same sprint, once the comparison shows no unfiled gap.

### Acceptance criteria

## Acceptance Criteria
- [ ] `sc-lint lint sc-runtime --config sc-lint.toml` exits 0 at the head; `analyzers.runtime` is set in `.sc-lint/install.json`.
- [ ] `.just/lint_runtime_waits.py`, its tests and its `run_lint.py` target are gone; `rg -n 'sc-lint-boundary' .just Justfile` finds no `cargo run -p` caller (bg-2 and bg-3 deleted the other wrappers).
- [ ] REQ-P-LINT-POSTMORTEM-001 and `docs/architecture.md` no longer require the embedded crates.
- [ ] `cargo metadata --format-version 1 --no-deps` lists no workspace member sc-lint-boundary or sc-lint-directives; both directories are gone; Cargo.lock has no path-sourced `sc-lint-*` package; `cargo build --workspace` passes.

## Validation
- `just lint`
- `just test`
- `cargo build --workspace`

---

## atm-bg-5: bg-5: Published sc_lint Python tools

## Goal
Every step the `sc_lint` wheel duplicates runs the published module (ANC-010): fmt, function-length, lines, manifests fully; spell, deny, shear, version and identities where the wheel covers them. Each legacy script is deleted after comparison (ANC-011, ANC-012); length findings are fixed by refactors (ANC-014).

## Deliverables
1. BG5-0 Invocation: `function-length`, `lines` and `identities` use `sc-lint lint function-length|line-counts|identity-literals`; `fmt` runs `cargo fmt --all --check` (the command the published fmt step runs); the other wheel modules run through one Justfile recipe that calls `-m sc_lint.<module>` with the `.sc-lint/venv` interpreter for the host OS, marked `delete when randlee/sc-lint#198 ships`. No second sc-lint pin in `tools/bootstrap.toml`.
2. BG5-1 Full duplicates: `fmt`, `function-length`, `lines`, `manifests` run as BG5-0 states; legacy scripts, their tests and the dead `modules` target deleted after comparison; `just fmt write|check` stays and calls `cargo fmt --all` directly (the pre-push hook and `just help` use it). For function-length, the comparison is: at the cut commit, the legacy tool's grandfathered list equals the rows `--write-baseline` writes; one finding bead tracks the baseline and closes when the file is empty.
3. BG5-2 Partial duplicates: `spell`, `deny`, `shear`, `version`, `identities` run the published tool; the gaps (sc-lint#349 Windows console output for spell and any other wheel runner that fails the same way on Windows, #346 shear allowlist, #347 identity `concat!`, #137 Python pin lockstep, and the deny DNS retry under a new sc-lint issue) stay as the smallest atm-side check each, marked `delete when randlee/sc-lint#<n> ships`.
4. BG5-3 Findings: each new function-length or line-count finding is fixed by a refactor that improves the code (no shaving to just under the limit); suppressions carry a reason and a finding bead.
5. BG5-4 Importers: `lint_manifests.py` (which imports `check_version_sync`) is deleted; `check_version_sync.py` stays as the version gap check, so `prerelease_tag.py` and `lint_hermes_atm_boundary.py` keep their imports; `view_lines.py` reads the published `line-counts` output or is deleted with its `just view lines` recipe (no copy of the line-count logic stays); `.just/tests/test_lint_external_tools.py` keeps only tests of the kept gap checks.
6. BG5-5 References: `lines` becomes the RULE-003 authority. Every live reference to a deleted script (the docs and agent files in owned_paths) points to its sc-lint step; historical records (`docs/plans/**`, `CHANGELOG.md`, `release/**`, `site/**`, `.triage/**`, `.sprints/**`, `reports/**`) are left as written.

### Design

## Rules every package step follows (ANC-011, 013, 014, 016, 019, 020, 021)
- Compare before delete: at one commit, run the legacy tool and its sc-lint replacement, and put both commands and the finding-set diff in the PR body. A finding the legacy tool reports that sc-lint does not is a gap: file it on randlee/sc-lint (general-purpose wording, no atm-core specifics) and send the issue number to team-lead@sc-lint the same day; keep the legacy check only for that finding, with `# delete when randlee/sc-lint#<n> ships`.
- New sc-lint findings are fixed in code. A length finding gets a refactor that makes the code clearer; moving lines to land just under a limit is rejected in review.
- A suppression attribute or allowlist entry is allowed only with a stated reason; each one added to get CI green gets a finding bead under the phase root in the same PR.
- Delete the legacy tool, its tests and its run_lint.py target in the same sprint, once the comparison shows no unfiled gap.

### Acceptance criteria

## Acceptance Criteria
- [ ] The full-duplicate scripts, their tests and `run_lint.py` targets are deleted; each partial keeps only its gap check with a `delete when randlee/sc-lint#<n> ships` line naming a real issue number.
- [ ] `rg --hidden -n '<deleted script name>'` outside the historical paths in BG5-5 returns nothing for every deleted script, and `just test` imports no deleted module.
- [ ] `tools/bootstrap.toml` has no sc-lint pin.
- [ ] The function-length baseline has one finding bead.

## Validation
- `just lint`
- `just test`

---

## atm-bg-6: bg-6: Runner cutover, platform scope, test manifest, speed

## Goal
sc-lint owns the step list and runs it: parallel, keep-going (sc-lint#195); `just lint ci` runs host/Linux-only and `just lint ci all` runs every platform (ANC-007, #199); `just test` is driven by the repo-owned manifest (ANC-008, #196); `just lint` is faster than at the phase base commit (ANC-004). `run_lint.py` shrinks to the atm-only steps' commands (ANC-017); the file stays while the sc-publish action `.github/actions/setup-sc-lint/action.yml` (ADR-050, not edited here) asserts it exists, and that dependency is filed on sc-publish.

## Dispatch gate
- `atm-phase-bg-gate-sclint-runner`: a published sc-lint release with #195, #196 and #199.

## Deliverables
1. BG6-1 Steps call their tool directly from `.sc-lint/install.json`; `run_lint.py`'s `all`/`fast` modes and log plumbing are deleted; CI and `scripts/validate_release.py` (which runs `python3 .just/run_lint.py all`) switch to `just lint ci all`. The `_py` and venv-interpreter recipes stay until sc-lint#198 ships, marked `delete when`.
2. BG6-2 Platform scope (REQ-P-PLATFORM-001/002): `just lint ci` = host-only, `just lint ci all` = every platform (#199). `clippy-windows` keeps its PYO3_PYTHON wrapper until #198 and its per-step target until #197, each marked `delete when`.
3. BG6-3 Test manifest: `just test` and its named selectors come from `test` in `.sc-lint/install.json` (the consumer manifest of #196), covering today's `just test` steps, including the `sign_daemon_dev.py` step (REQ-P-TEST-001); plain `just test` excludes coverage; `just test coverage` writes the host's tracked report on macOS and Windows and fails clearly on Linux (REQ-P-COVERAGE-001).
4. BG6-4 Speed (ANC-004): on one host with warm caches (cargo target dir, venvs), time the same check set at the phase base commit (`just lint`, i.e. `run_lint.py all`) and at the bg-6 head (`just lint` with `clippy-windows` excluded, the one step the base does not run): 1 discarded run, then 3 timed runs each; the head median is lower; commands and numbers in the PR body.
5. BG6-5 Docs: `docs/lint-suite-prd.md` and `just help` describe the new commands.

### Design

## Rules
- A suppression added to get CI green gets a finding bead under the phase root in the same PR (ANC-016).

### Acceptance criteria

## Acceptance Criteria
- [ ] `just lint ci` and `just lint ci all` pass on Linux CI; `ci all` includes `clippy-windows` on non-Windows hosts.
- [ ] `rg -n 'run_lint.py all' .github scripts` returns nothing.
- [ ] A failing step does not hide later steps: every failure is reported in one run.
- [ ] `just test` runs from `test` in `.sc-lint/install.json`; plain `just test` excludes coverage; `just test coverage` fails clearly on Linux.
- [ ] The BG6-4 measurement shows the head median below the base commit's.

## Validation
- `just setup && just lint ci all`
- `just test` and `just test coverage`
