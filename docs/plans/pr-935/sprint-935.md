---
id: pr-935
title: Adopt shared sc-compose publish kit
status: complete
branch: feature/vendor-sc-compose-publishing-skill
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/vendor-sc-compose-publishing-skill
---

# PR #935 — Adopt shared sc-compose publish kit

## Background

Rand's direction: atm-core should publish through the same unified,
manifest-driven publish kit shared across all repos, sourced from
`sc-compose`, rather than maintaining a bespoke release pipeline. The
canonical upstream is sc-compose PR #507
(`feature/publish-kit-preflight-hardening`, local worktree at
`/Users/randlee/Documents/github/sc-compose-worktrees/review-pr-507`), whose
normative spec is
`/Users/randlee/Documents/github/sc-compose-worktrees/review-pr-507/docs/publish-kit-requirements.md`.
This whole vendoring approach is explicitly temporary, until the publish kit
is available via a marketplace — at that point the sync mechanism goes away
in favor of normal marketplace install/update.

## Goals

- vendor the sc-compose publish kit (PR #507 @ `325183c`) into atm-core:
  manifest-driven release destinations, shared release-gate/release-artifacts
  scripts, shared credential preflight
- move Homebrew/Scoop/winget/crates.io destination facts into
  `release/publish-artifacts.toml`, not hardcoded in workflow YAML
- add a new Scoop publish channel (new destination, not previously supported)
- add manifest-selected multi-binary Homebrew formula support — atm-core's
  formulas (`agent-team-mail.rb`, `atm.rb`) install both the `atm` and
  `atm-daemon` binaries; PR-507's original template only modeled one binary
  per formula, which would have silently dropped the daemon if adopted as-is
- adopt PR-507's explicit `--version`-arg release gating, while preserving
  ATM's tag-immutability hard-fail (an existing release tag pointing at a
  different commit than the one being released must still hard-fail) — this
  protection is directly tied to a real incident this session: `v1.4.2` had
  to be abandoned and recovered as `v1.4.3` after a packaging fix landed on
  `main` instead of the release branch, and the tag-immutability rule is what
  made that failure mode visible and unrecoverable-in-place by design (never
  retag)
- centralize duplicated tool-install logic (e.g. `ci.yml` previously pinned
  the `sc-compose` CLI to two different revisions across jobs despite a
  comment claiming parity — a confirmed, real, previously-unfixed bug) into a
  shared composite action
- consolidate two overlapping temporary vendor-sync scripts into one
  hardcoded-file-list + source-revision + `--check` mechanism

## Scope

- in scope:
  - `.github/workflows/release.yml`, `release-preflight.yml`,
    `hermes-atm-pypi-publish.yml`
  - `scripts/release_gate.sh`, `scripts/release_artifacts.py`
  - `scripts/release_manifest.py` (new, vendored from PR-507)
  - `scripts/sync_sc_compose_publish_kit.py` (new, consolidates the two prior
    sync scripts)
  - `release/publish-artifacts.toml`, `release/publish-channel-contracts.toml`
  - `release/homebrew/*.rb.j2` templates
  - `.claude/skills/publishing/` (vendored skill)
  - `.claude/agents/*-publisher.md` (vendored agent prompts)
- out of scope:
  - dispatching, tagging, or publishing any actual release as part of this
    work (tooling/workflow changes only — matches sc-compose's own
    `publish-kit-requirements.md` section 7 scope boundary)
  - PyPI publish for `hermes-atm`/`atm-graft` at 1.4.3 (separate, already
    tracked, not part of this PR)
  - generic dynamic Maturin/setuptools Python-distribution support (real gap,
    already reported upstream to comp as follow-up, not blocking here)

## Retained ATM-specific validation (not to be removed)

`scripts/validate_release.py`, `scripts/verify_release_archive.py`, and
`scripts/prepare_hermes_atm_publish_artifacts.py` have no sc-compose
equivalent and encode genuinely atm-core-specific concerns (cargo-lock-drift,
phase-ad-readiness, release-binaries/installed-doc archive-membership
guarantees, hermes-atm PyPI two-distribution packaging). These must remain
present and wired into the adopted release flow, not deleted as
"duplicated."

## Required Validation

- `just lint`
- `just test` (Python + Rust workspace)
- release/tooling/version-specific test modules
- confirm `scripts/release_gate.sh` still hard-fails when an existing release
  tag points at a commit other than the one being released, with real test
  coverage (not just claimed)
- confirm both `agent-team-mail.rb` and `atm.rb` generated Homebrew formula
  content installs both `atm` and `atm-daemon`
- confirm no repo-literal channel/destination values leak back into workflow
  YAML — everything routes through `release/publish-artifacts.toml`
- confirm no `workflow_dispatch`, tag, or actual publish action occurred as
  part of this change

## Note on `docs/project-plan.md`

This is an ad hoc infrastructure PR, not a numbered sprint in the phased
project plan (`docs/project-plan.md`). There is intentionally no
`docs/project-plan.md` entry for this work — do not treat its absence as a
gap.
