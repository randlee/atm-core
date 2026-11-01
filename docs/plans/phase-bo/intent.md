# Intent - phase-bo (restart)

Rewritten 2026-11-01 by fenix for the phase-bo restart. One page at most. This is the "why" that frames every anchor; Jev judges plans and work against the anchors in `anchors.json`, not against this page.

## What this plan is for

atm-bd-orchestration runs a full phase pipeline: dev, dev-sanity, stack link, QA, fix rounds, landing. That is right for a large agent team and wrong for one agent or a small team. Rand wants an `atm-bd-lite` profile: the same bead state model, schema and `atm-bd` command interfaces, with a dev -> QA pipeline and no sanity stage, run by one agent.

The first attempt (bo-1..bo-7) broke atm-bd-orchestration: it moved orchestration's scripts and templates into a new `core/` layer and rewired orchestration onto it. That is a breaking refactor and is not wanted. Rand, 2026-11-01: "atm-bd lite was supposed to be separate w/ shared code in a shared location. (atm-bd-orchestration was NOT supposed to change to use the 'shared' code. the 'shared' code was in shared/ location by design not refactor."

This restart:

1. Repairs the current stack first: everything that breaks atm-bd-orchestration is removed, so orchestration is byte-identical to its released state; the published `atm-bd-types` crate stays, and shared code already written stays if it is new code, valid and simple.
2. Builds lite on top: lite-specific code in `skills/atm-bd-lite/`, everything not lite-specific in the shared location as new code.
3. Makes the shared code real shared code before anyone cuts over to it. Lite uses each shared module the sprint it lands. Orchestration stays untouched and cuts over in a later phase. Each shared module is designed now to the behaviors orchestration's existing code has today (an inventory citing orchestration's files and functions), and tests run it on real full-profile data and show it gives the same output as orchestration's unchanged code. Rand: "If you don't do this, you will not have shared code that works WHEN we cut over and use it."
4. Keeps lite tight: the lite scripts are small, pyright strict, and built on `atm-bd-types` with its interfaces as Rust traits (Python implementations checked against generated `.pyi` stubs). The three ATM task hooks (task.assign, task.start, task.close) are three independent files, each with its own state-transition diagram, and Jev verifies each file matches its diagram 100%, both directions.

## What success looks like

- The stack top leaves atm-bd-orchestration exactly as released, and its existing tests pass with no test edited.
- A phase file declares `profile = "lite"` and one agent runs a lite phase end to end from release assets: dev dispatched, dev closed by the lite task.close hook (git-only stack check, bd close, QA assigned), QA closed, the stack landed.
- Every shared module has full-profile tests and same-output tests against orchestration, so the later cutover is a switch, not a rewrite.

## Deliberately out of scope

- Changing atm-bd-orchestration, its close hook or its sanity pipeline, including cutting it over to the shared code.
- A live Azure phase (no Azure repo here; Azure is render- and unit-level only).
- Mixing profiles in one phase.

## Anchors

`anchors.json` is the record of truth, rendered to `anchors.md`. Retired anchors stay for history.
