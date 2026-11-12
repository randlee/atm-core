# Intent - phase-bg: sc-lint 0.8.8 published-crate migration

Written 2026-10-11 by fenix, from Rand's statements of 2026-10-09 and 2026-10-11. Jev judges the anchors, not this page.

## What this plan is for

atm-core moves its lint onto the published sc-lint crates (0.8.8) and removes every local lint tool that sc-lint duplicates. Rand: "the intent is to migrate to sc-lint published crates and eliminate all unnecessary local lint tools that are duplicated on sc-lint." atm-core gets the same `just` workflows as the other sc repos, through the sc-lint adoption kit, with its own needs expressed as kit config or upstream extension-point requests.

Legacy tools are phased out, not cut over blind: each runs beside its sc-lint replacement until their output is compared, then the duplicate is deleted. Code that sc-lint flags is fixed properly, with a small refactor where length limits fire. Suppression attributes are a last resort to keep CI green, and each one gets a findings bead.

The move is swift and decisive (Rand: "I also expect the move to sc-lint packages to be swift and decisive"): one package at a time, each gap filed upstream as it is found.

This replaces the stalled phase-be plan, which waited on sc-lint 0.6.x releases.

## What success looks like

- `just lint` runs on the published sc-lint 0.8.8, with no vendored sc-lint crates left in the workspace.
- Every legacy lint tool that sc-lint duplicates is deleted, after a recorded output comparison.
- `just lint` runs faster than before and loses no check without Rand's acceptance.
- sc-lint findings are fixed in code; every remaining suppression attribute has an open findings bead.

## What is deliberately out of scope

- Changes to sc-lint itself: a gap becomes an issue on randlee/sc-lint.
- Lint checks unique to atm-core: they stay as atm-core steps.
- Product code changes beyond what fixing sc-lint findings needs.

## Later ruling for the bg-7 PE-9 non-fix

The `atm-phase-bg` root bead's pause note (`PAUSED 2026-10-27T01:35Z`)
records Rand's ruling: “I don't want your implementation of anything.” The
same note records the scope: atm-core owns ATM concerns, sc-lint owns lint
concerns, and atm-core must not implement lint. This is the ruling behind
PE-9's non-fix disposition; the PE-9 close reason's `(Rand 2026-10-26)`
shorthand omitted the quoted words. Source: `bd show atm-phase-bg`, Notes,
`PAUSED 2026-10-27T01:35Z` entry.

## Anchors

Anchors for this plan live in `anchors.json` (record of truth) and are rendered to `anchors.md`.
