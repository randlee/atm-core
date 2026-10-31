# Intent - phase-bo

Written 2026-10-31 by fenix. One page at most. This is the "why" that frames every anchor beneath it; Jev questions are judged against anchors, not against this document, so keep it short and let the anchors carry the testable facts.

## What this plan is for

atm-bd-orchestration runs a full phase pipeline: dev, dev-sanity (Jev + LLM), stack link, QA, fix rounds, landing. That pipeline is right for a large agent team and wrong for a single agent or a small team, where the sanity stage and its ~10k lines of code and tests are overhead with no consumer. Rand wants an `atm-bd-lite` profile: the same bead state model and schema, the same tool interfaces (`atm-bd console`, `atm-bd cron`, `atm-bd queues`), but a much simpler dev -> QA transition driven by its own small task-close hook, written as one Python file organized by transition type with exhaustive enum checks.

Two coupled problems ride along. First, the package duplicates its scripts and templates into `.claude/`, `.codex/` and `.agents/` on install; the profiles need one shared `atm-bd-core` layer at a common point (`.atm-bd/scripts`, `.atm-bd/templates`) with only the profile-specific hook, assignment templates, cron and formulas living under each profile skill. Second, stacking depends on the `gh stack` extension and the GitHub stacks API, so the pipeline cannot run against Azure Git repos. Rand ruled that both profiles check stacking by comparing the PR base with the stack top (git only, blocking the dev/fix close), never through the `gh stack` subcommand, and that provider-specific commands (`gh` vs `az repos`) are injected into the sc-compose templates by an installer flag (`--github` / `--azure`).

## What success looks like

- A phase file declares `profile = "lite"` and a single agent runs a lite phase end to end from GitHub release assets: dev bead dispatched, dev task closed by the lite hook (stack check -> bd close -> link layer -> QA assigned), QA closed, the stack landed, with `atm-bd console`, `atm-bd cron` and `atm-bd queues` showing the phase correctly.
- Shared types, schema, render helpers and tool entry points live in `atm-bd-core` and are installed once to `.atm-bd/scripts` and `.atm-bd/templates`; profile skills carry only their own hook, templates, cron tick and formulas.
- Neither profile calls `gh stack` or the GitHub stacks API; the stack gate is `git merge-base` plus the PR base ref, and the Azure provider renders the same templates with `az repos` commands.
- The full profile (`atm-bd-orchestration`) keeps its behaviour and its tests green on the new layout.

## What is deliberately out of scope

- Redesigning the full profile's sanity pipeline or its close hook (phase bm scope).
- Running a live Azure phase: no Azure repo exists in this environment, so Azure acceptance is render-level and unit-level only.
- The TUI modularity work and the `quick_fix` flow.
- Mixing profiles inside one phase (ruled out by Rand).

## Anchors

Anchors for this plan live in `anchors.json` (record of truth) and are rendered to `anchors.md`. Requirements and ADRs that govern this plan are listed there too, under their own IDs.
