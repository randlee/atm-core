---
id: issue-522
title: Homebrew Formula Platform Asset Mapping Fix
status: complete
branch: fix/issue-522-homebrew-formula-platform-assets
worktree: /Users/randlee/Documents/github/atm-core-worktrees/fix/issue-522-homebrew-formula-platform-assets
---

# Sprint 522 — Homebrew Formula Platform Asset Mapping Fix

## Goals

- stop `update-homebrew` from stamping every Homebrew platform block with the
  Apple Silicon macOS archive and checksum
- generate distinct URL + `sha256` pairs for:
  - `on_macos/on_arm` -> `aarch64-apple-darwin`
  - `on_macos/on_intel` -> `x86_64-apple-darwin`
  - `on_linux/on_intel` -> `x86_64-unknown-linux-gnu`
- add a ratchet that validates generated formulas against the release
  archives/checksums inside the actual release pipeline so the mismatch cannot
  silently regress
- cover the formula generator and mismatch detection with unit tests that do
  not require live GitHub access

## Scope

- in scope:
  - `scripts/release_artifacts.py`
  - `.github/workflows/release.yml`
  - release validation/unit tests under `.just/tests/`
  - release wiring lint fragments under `.just/lint-config.toml`
- out of scope:
  - correcting already-published formulas in `randlee/homebrew-tap`
  - changing Windows release/publish behavior

## Implementation Plan

- move Homebrew formula mutation out of ad hoc workflow `sed` commands into
  `scripts/release_artifacts.py`
- teach the script two Homebrew-specific commands:
  - `update-homebrew-formulas`
  - `validate-homebrew-formulas`
- drive both commands from the release job using the archives downloaded into
  `release/` plus `release/checksums.txt`
- validate both URL target triple selection and `sha256` selection against the
  release archive checksum inventory
- preserve the current formula file structure; only rewrite:
  - top-level `version`
  - platform-specific `url`
  - platform-specific `sha256`

## Release Pipeline Call Site

- release ratchet is wired into `.github/workflows/release.yml` under the
  `update-homebrew` job
- that job now:
  - checks out the tagged `atm-core` source
  - downloads the release artifacts
  - regenerates `release/checksums.txt`
  - runs `python3 scripts/release_artifacts.py update-homebrew-formulas`
  - immediately runs
    `python3 scripts/release_artifacts.py validate-homebrew-formulas`
- the validation step consumes the same downloaded release archives/checksums
  the job is about to publish into the tap, so a target-triple or checksum
  mismatch fails the release job before the tap commit/push step

## Required Deliverables

- `docs/plans/issue-522/sprint-522.md` exists with frontmatter
- Homebrew formula updater maps each platform block to its correct target
  triple archive and checksum
- a validator fails when any formula block points at the wrong target triple
  or wrong checksum
- the validator is called from `.github/workflows/release.yml` in the
  `update-homebrew` job after formulas are updated
- unit tests cover:
  - correct per-platform rewrite
  - mismatch detection that would fail on the old all-arm formula state
- `just test` passes
- `just lint` passes

## Acceptance Criteria

- generated formulas contain three distinct archive targets across:
  - macOS ARM
  - macOS Intel
  - Linux Intel
- no platform block besides `on_macos/on_arm` references
  `*_aarch64-apple-darwin.tar.gz`
- the release workflow call site uses the repo script rather than inline
  global `sed` replacement for URL/sha mutation
- the release workflow contains an explicit validation step that can fail the
  release if Homebrew formulas no longer match the downloaded release
  archives/checksums
- this sprint explicitly records that repairing already-published 1.2.3/1.3.0
  formulas in `randlee/homebrew-tap` remains a separate manual/user follow-up

## Follow-Up Left Out Of Scope

- backfill/correct the already-published Homebrew formulas in the external
  `randlee/homebrew-tap` repository after this generator fix merges
