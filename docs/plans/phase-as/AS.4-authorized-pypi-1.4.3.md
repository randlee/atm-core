# AS.4 — Authorized PyPI 1.4.3 publication

```yaml
plan_type: sprint_plan
phase: AS
sprint: AS.4
worktree: main immutable release assets; canonical publisher agents from AS worktree
branch: main
status: proposed
estimated_scope: one authorized production channel
```

## Goal

Publish only the already-built `1.4.3` Python artifacts from immutable `main`
through the canonical PyPI channel and retain a public verification receipt.

## Scope Summary

This is an authorized production operation, not a rebuild or full release.
The canonical agents may be launched from the Phase AS branch, but artifacts
and workflow source are pinned to immutable `main`.

## Governing Requirements

- Explicit production authorization is mandatory.
- Publication uses attached immutable assets and the AS.3-matched receipt.
- Python artifact verification covers 3.11, 3.12, 3.13, and 3.14.

## Governing ADRs

- No new ADR.

## Governing Boundaries

- Only PyPI is enabled. Crates, tag, GitHub Release, Homebrew, Scoop, winget,
  and other channels are excluded.

## Prerequisites

- AS.3 evidence accepted.
- Exact immutable `main` commit/tag and manifest-matching 1.4.3 artifacts.
- Human production authorization.

## Hard Dependencies

- `AS.3`: `must_follow`; receipt digest must match.
- `AS.5`: `must_follow`; migration merge waits for publication proof.

## Non-Goals

- Rebuilding artifacts, recreating tags, publishing crates, or version bumping.

## Sub-Tasks

1. Verify artifact checksums, version, and main commit against the approved
   manifest and AS.3 receipt.
2. Execute only the canonical PyPI channel.
3. Query the public package index and install/verify each supported Python
   version from published artifacts.
4. Store channel and public verification receipts.

## Split Recommendation

Keep this one-channel release separate from AS.6’s full 1.4.4 release.

## Acceptance Criteria

- Only the authorized PyPI channel ran.
- Public PyPI exposes the approved 1.4.3 artifacts.
- Python 3.11–3.14 verification succeeds.
- Receipts prove immutable source/artifact identity and public availability.

## Required Validation

```bash
python3 scripts/release_artifacts.py verify-atm-graft-wheel --help
python3 -m pip install --only-binary=:all: hermes-atm==1.4.3
```

## Required Document Updates

- Record authorization, workflow run, artifact digests, index URLs, and Python
  matrix evidence.

## Risks And Watchouts

Existing target versions must be classified with PF-3 semantics; do not treat
an already-published version as permission to rebuild or overwrite it.
