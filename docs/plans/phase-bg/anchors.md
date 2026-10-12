# Anchors - phase-bg

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `doc/plan/phase-current/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | locked | Same just workflows | 2026-10-09 |
| ANC-002 | locked | Extension points, not kit injection | 2026-10-09 |
| ANC-003 | locked | Lint as good or better | 2026-10-09 |
| ANC-004 | locked | Lint faster | 2026-10-09 |
| ANC-005 | locked | Windows lint via xwin | 2026-10-09 |
| ANC-006 | retired | Cross-platform lint | 2026-10-09 |
| ANC-007 | locked | just lint ci: Linux or all | 2026-10-09 |
| ANC-008 | locked | just test from a manifest | 2026-10-09 |
| ANC-009 | locked | Analyzers configured independently | 2026-10-09 |
| ANC-010 | locked | Published sc-lint crates | 2026-10-09 |
| ANC-011 | locked | Compare before removal | 2026-10-11 |
| ANC-012 | locked | Remove duplicate legacy tools | 2026-10-11 |
| ANC-013 | locked | Fix sc-lint findings | 2026-10-11 |
| ANC-014 | locked | Refactor, don't squeeze | 2026-10-11 |
| ANC-015 | retired | No attribute suppression of fixable warnings | 2026-10-11 |
| ANC-016 | locked | Suppression gets a findings bead | 2026-10-11 |
| ANC-017 | locked | Small, maintainable sc-lint support code | 2026-10-11 |
| ANC-018 | locked | Boilerplate goes upstream | 2026-10-11 |
| ANC-019 | locked | One package at a time | 2026-10-11 |
| ANC-020 | locked | Gaps filed on sc-lint, team-lead notified | 2026-10-11 |
| ANC-021 | locked | sc-lint stays general purpose | 2026-10-11 |

## ANC-001

Anchor: atm-core must use the same just-defined workflows as the other sc repos: just setup, just lint, just test and just upgrade come from the sc-lint adoption kit.

> it is very difficult trying to move from 3 repos to 10 that I want set up w/ the SAME just defined workflows

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: locked. Version 2.

Summary: Same just workflows

Scope: any bead that adds, edits or removes a just recipe or the Justfile

Fixtures:
- pass: Install the sc-lint adoption kit; just setup, just lint, just test and just upgrade come from its managed import in the Justfile.
- fail: Keep atm-core's own just lint and just test recipes and add sc-lint as one more step inside them.

## ANC-002

Anchor: atm-core must request per-repo extension points and repo-specific manifest/config from the sc-lint kit instead of putting repo-specific logic in kit-managed files.

> I have found using a shared kit becomes a very big problem w/ sc-publish as every repo consuming the kit injects repo specific logic (instead of requesting per repo extension points and repo specific manifest/config)

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: locked. Version 4.

Summary: Extension points, not kit injection

Scope: any bead that installs, upgrades or edits sc-lint kit files or needs atm-core-specific lint or test behaviour

Fixtures:
- pass: atm-core's daemon-test steps are declared in its own sc-lint.toml, and the hook the kit lacks is requested on randlee/sc-lint as a per-repo extension point; no kit-managed file is edited.
- fail: Patch the kit's managed justfile to add an atm-core-specific branch for the daemon tests.

## ANC-003

Anchor: Every lint check atm-core runs today must have an sc-lint equivalent, or Rand must accept its removal.

> I expect sc-lint (repo) to have as good or better lint tools and higher performance than what we currently have on atm.

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: locked. Version 2.

Summary: Lint as good or better

Scope: any bead that removes, replaces or retargets an atm-core lint check

Fixtures:
- pass: Every current just lint check is mapped to its sc-lint replacement or kept as an atm-core step; none is dropped.
- fail: Drop the RULE-003 line-count check because sc-lint has no equivalent yet.

## ANC-004

Anchor: The migrated just lint must run faster than atm-core's current just lint on the same host.

> I expect sc-lint (repo) to have as good or better lint tools and higher performance than what we currently have on atm.

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: locked. Version 2.

Summary: Lint faster

Scope: any bead that changes how just lint runs

Fixtures:
- pass: Measure just lint wall time before and after on the same host and tree; the migrated run is faster.
- fail: Accept a slower just lint since sc-lint does more checks.

## ANC-005

Anchor: Windows-target lint must run on macOS and Linux hosts through xwin.

> yes, cross platform support (use of xwin for non-windows conosumers) and cross platform lint.

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: locked. Version 4.

Summary: Windows lint via xwin

Scope: any bead that configures cross-target lint or clippy targets

Fixtures:
- pass: Windows-target lint, including clippy, runs on macOS and Linux hosts through cargo-xwin.
- fail: Windows-target clippy runs only on the Windows CI runner.

## ANC-006

Anchor: Lint must run on Linux, macOS and Windows.

> yes, cross platform support (use of xwin for non-windows conosumers) and cross platform lint.

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: retired. Version 3.

Summary: Cross-platform lint

Scope: any bead that configures which platforms lint runs on

Fixtures:
- pass: The lint job runs on Linux, macOS and Windows.
- fail: Lint runs on Linux only; macOS and Windows are skipped.

## ANC-007

Anchor: `just lint ci` must offer both a Linux-only run and an all-platforms run.

> running just linux or all platforms should both be options for `just lint ci`

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: locked. Version 2.

Summary: just lint ci: Linux or all

Scope: any bead that defines just lint ci or the CI lint workflow

Fixtures:
- pass: `just lint ci` takes a scope: linux runs only Linux, all runs Linux, macOS and Windows.
- fail: `just lint ci` always runs all three platforms; there is no Linux-only option.

## ANC-008

Anchor: `just test` must be driven by a repo-owned manifest.

> 'just test' in general should work off a repo specific manifest for extensibility and customization.

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: locked. Version 2.

Summary: just test from a manifest

Scope: any bead that defines or changes just test

Fixtures:
- pass: `just test` reads its selectors and defaults from atm-core's test manifest.
- fail: `just test` hard-codes the cargo test invocations in the recipe.

## ANC-009

Anchor: Each sc-lint analyzer atm-core runs must have its own configuration table.

> every analyzer shold be independenty configured.

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: locked. Version 2.

Summary: Analyzers configured independently

Scope: any bead that configures an sc-lint analyzer

Fixtures:
- pass: Each analyzer is configured in its own [tool.sc-lint.analyzers.<name>] table.
- fail: All analyzers share one settings table in sc-lint.toml.

## ANC-010

Anchor: atm-core's lint must use the published sc-lint crates.

> can you review any recent plans to migrate our sc-lint to use the published sc-lint crates?

Said by Rand, planning session (atm-core fenix), 2026-10-09. Status: locked. Version 3.

Summary: Published sc-lint crates

Scope: any bead that touches vendored sc-lint crates or the sc-lint version atm-core uses

Fixtures:
- pass: Install sc-lint 0.7.0 from its published release and delete the vendored sc-lint crates from the workspace.
- fail: Keep building the vendored sc-lint crates from the workspace and pin them at 0.7.0.

## ANC-011

Anchor: Each legacy atm-core lint tool must run alongside its sc-lint replacement, with their outputs compared, before the legacy tool is removed.

> You should phase them out so you can compare output

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 2.

Summary: Compare before removal

Scope: any bead that replaces or removes a legacy lint tool

Fixtures:
- pass: Run the legacy boundary check and sc-lint boundary side by side, diff their findings, and record the comparison before deleting the legacy check.
- fail: Replace the legacy boundary check with sc-lint boundary in one step without comparing their findings.

## ANC-012

Anchor: After its sc-lint replacement is in place, every legacy atm-core lint tool that sc-lint duplicates, the vendored sc-lint crates included, must be removed.

> after replacement, I want all legacy lint tools that are duplicate removed.

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 3.

Summary: Remove duplicate legacy tools

Scope: any bead that replaces a legacy lint tool or closes the migration

Fixtures:
- pass: After the comparison, delete .just/check_line_counts.py since sc-lint's length check replaces it.
- fail: Keep the legacy check scripts in .just/ alongside sc-lint as a fallback.

## ANC-013

Anchor: Every code issue sc-lint flags must be fixed in code unless it needs to be ignored for a stated good reason.

> I also expect that any code issues flagged by sc-lint to NOT be ignored unless they NEED to be ignored for good reason.

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 2.

Summary: Fix sc-lint findings

Scope: any bead that enables an sc-lint check or handles its findings

Fixtures:
- pass: Fix each sc-lint finding in the code; the one ignore kept has its reason recorded beside it.
- fail: Add the existing sc-lint findings to a baseline file so they are ignored.

## ANC-014

Anchor: A lines-of-code lint finding must be fixed by a small refactor that cleans the code up well, not by moving code to land just under the limit.

> if lines of code triggers lint, plan a small refactor to clean this up well.  (don't move things so you are 1-line under the limit)

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 2.

Summary: Refactor, don't squeeze

Scope: any bead that handles a file-length or function-length finding

Fixtures:
- pass: Split the oversized function into two helpers with clear responsibilities.
- fail: Join statements onto fewer lines so the function is one line under the limit.

## ANC-015

Anchor: Warnings that should be fixed must not be disabled with attributes.

> also do not disable warnings w/ attributes for things that should be fixed.

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: retired. Version 5.

Summary: No attribute suppression of fixable warnings

Scope: any bead that adds an allow/expect attribute or other lint suppression

Fixtures:
- pass: Fix the clippy warnings in the code instead of adding #[allow] attributes for them.
- fail: Add #[allow(dead_code)] to silence the warnings in the new module.

## ANC-016

Anchor: Every warning disabled with an attribute so CI passes must have a findings bead to fix it later.

> if you need to disable things w/ attributes so ci passes, create a findings bead and fix it later.

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 2.

Summary: Suppression gets a findings bead

Scope: any bead that adds a lint suppression attribute

Fixtures:
- pass: The #[allow(clippy::too_many_lines)] added so CI passes has findings bead atm-xyz to remove it.
- fail: Add #[allow(clippy::too_many_lines)] so CI passes, with no follow-up recorded.

## ANC-017

Anchor: atm-core's code supporting sc-lint must be small and easily maintainable.

> I also expect the resulting code supporting sc-lint to be small and easily maintainable.

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 3.

Summary: Small, maintainable sc-lint support code

Scope: any bead that adds or changes atm-core code, config or recipes that support sc-lint

Fixtures:
- pass: Keep atm-core's sc-lint support small and easy to maintain: only its sc-lint.toml and two short recipes outside the kit block, no custom driver code.
- fail: Add a 600-line .just/sclint_driver.py that re-implements step dispatch and report merging for atm-core.

## ANC-018

Anchor: Where atm-core needs boilerplate to use sc-lint, the plan must consider filing an issue on randlee/sc-lint for sc-compose j2 templates or consumer-facing code that removes that boilerplate.

> if boilerplate is necessary, think about filing a gh to sc-lint to create sc-compose j2 templates or add code to make the tools easy for consumers to use.

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 2.

Summary: Boilerplate goes upstream

Scope: any bead that adds boilerplate atm-core needs to use sc-lint

Fixtures:
- pass: The per-analyzer config tables are boilerplate, so file an issue on randlee/sc-lint asking for an sc-compose j2 template that renders them for consumers.
- fail: Hand-write the repeated per-analyzer config boilerplate in atm-core and copy it to each new repo.

## ANC-019

Anchor: The migration must replace atm-core's lint packages with sc-lint packages one package per step, each step recording the gaps and errors it found.

> migrate and replace packages one at a time, noting any gaps or errors.

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 2.

Summary: One package at a time

Scope: any bead that migrates a lint tool or package to sc-lint

Fixtures:
- pass: Step 1 replaces only the boundary analyzer with sc-lint boundary and records the two gaps it found; portability follows in step 2.
- fail: Replace boundary, portability, runtime-waits and the line-count check with sc-lint in one step.

## ANC-020

Anchor: Each sc-lint gap or error found during the migration must be filed as an issue on randlee/sc-lint and sent to team-lead@sc-lint when it is encountered.

> file issues on sc-lint when encountered and notify team-lead so he addresses them quickly (assuming they are valuable additions)

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 2.

Summary: Gaps filed on sc-lint, team-lead notified

Scope: any bead that finds an sc-lint gap or error during the migration

Fixtures:
- pass: The missing Windows prefix allowlist found while migrating portability is filed on randlee/sc-lint and sent to team-lead@sc-lint the same day.
- fail: Work around the missing allowlist with a local wrapper script and note it for later; nothing is filed upstream.

## ANC-021

Anchor: Every change atm-core asks of randlee/sc-lint must be a general-purpose capability; atm-core-specific lint checks stay in atm-core.

> sc-lint is a general purpose tool and atm-core specific concerns do not belong

Said by Rand, planning session (atm-core fenix), 2026-10-11. Status: locked. Version 2.

Summary: sc-lint stays general purpose

Scope: any bead that files an sc-lint issue or decides where an atm-core lint check lives

Fixtures:
- pass: File sc-lint issue: per-rule path allowlists configurable from sc-lint.toml, for any consumer; atm-core's daemon-signing coupling check stays an atm-core step.
- fail: File sc-lint issue: add an atm-daemon signing-coupling analyzer to sc-lint for atm-core.
