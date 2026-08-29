# Sprint AS1.1 — Prerelease archive job

**Branch**: `feature/ar1-prerelease-archive` → PR targets `develop`
**Worktree**: `../atm-core-worktrees/feature/ar1-prerelease-archive`
**Owner**: fenix (Phase AS (release validation 1.4.x)) · **Consumer**: loki@hermes docker-testbed and the
Mac daemon-switch install for cross-host parity.

## Purpose

Phase AS (release validation 1.4.x) needs CI-provenanced Linux, macOS, and Windows
archives with the same layout as the production release installer. The
workspace version is the release identity: `just prerelease-tag` patch-bumps
`X.Y.Z`, commits that bump on the operator's current branch, and pushes the
matching `prerelease/vX.Y.Z` tag. The tag push is the only trigger for
`.github/workflows/prerelease-archive.yml`.

The workflow builds archives and checksums but never creates a GitHub Release,
creates another tag, or publishes anything. A tag may come from any branch;
the workflow deliberately performs no develop-reachability check.

## Vendored sc-publish boundary

`release.yml` and `.github/scripts/release_artifacts.py` are pinned
sc-publish kit files and remain untouched. The atm-core-owned prerelease
workflow duplicates the production packaging block and uses the existing
`build-plan`, `verify-version`, `verify-version-lockstep`, `release-target-matrix`,
`cargo-build-bin-args`, and `release-package-config` subcommands. The
`.just/tests/test_prerelease_archive_workflow.py` compares the extracted
Python blocks byte-for-byte after normalizing only their version-output line.
No new kit subcommand is introduced.

## Delivered

1. `.github/workflows/prerelease-archive.yml` triggers only for
   `prerelease/v*.*.*` tag pushes. Its plan job validates the tag's stable
   `X.Y.Z` version against the workspace and all release-contract versions,
   rejects an existing GitHub Release for the tag, and resolves the complete
   manifest target matrix. Build jobs use the existing release commands and
   the byte-equivalent packaging block. The checksum job retains
   `checksums.txt` and `provenance.json` output. The workflow has only
   `contents: read` permission and does not test ancestry against `develop`.
2. `Justfile` exposes `just prerelease-tag`, backed by
   `.just/prerelease_tag.py`. It refuses `develop`, `main`, detached HEADs,
   dirty trees, duplicate tags, and non-stable workspace versions; updates
   workspace version fields, the three registered Python package manifests,
   configured Winget fields, and `Cargo.lock`; verifies lockstep; commits the
   bump on the current branch; then creates and pushes `prerelease/vX.Y.Z`.
   Python updates delegate to the release-artifacts kit's
   `sync-python-version`; Winget updates delegate to the config-owned
   `check_version_sync.py` implementation. `candidate_changes()` copies only
   `git ls-files` output, validates the candidate, and returns the exact
   version-manifest allowlist; `write_and_commit()` applies that set with
   rollback on commit failure.
   `--dry-run` prints the exact bump, commit, tag, and push trace without
   changing the checkout.
3. `.just/tests/test_prerelease_archive_workflow.py` is repository-owned,
   uses `lint_common.discover_repo_root()`, is collected by
   `.just/run_pytests.py`, and covers packaging equivalence, tag-only
   validation, no-tag/no-publish behavior, permissions, checksums, and the
   prerelease-tag recipe.

## Operator flow

From a clean feature or fix branch:

```bash
just prerelease-tag --dry-run
just prerelease-tag
gh run list --workflow prerelease-archive.yml --limit 1
gh run download <run-id> -n x86_64-unknown-linux-gnu
gh run download <run-id> -n checksums
shasum -a 256 -c checksums.txt
```

The second command patch-bumps the workspace (for example `1.4.5` to
`1.4.6`), commits and pushes that bump on the current branch, and pushes
`prerelease/v1.4.6`. The workflow then checks out the tag, so its archive
names and provenance use the same stable workspace version.

## Verification

- `python3 .just/run_pytests.py` — the workflow test is collected and passes;
  the focused prerelease suite contains 13 tests.
- `just lint` — full repository lint gate.
- `cargo test --workspace` — required when the recipe/version tooling changes
  Rust-facing workspace metadata or lockfile behavior.
- `just prerelease-tag --dry-run` — confirms the local branch/version bump,
  lockstep verification, commit, tag, and two push operations without making
  any changes.
- The vendored `.github/scripts/tests/` suite is not part of the local
  collector. Against current `develop` at `ae03b6a91`, its baseline is 7
  failures (publisher metadata version, renderer-path pin, release-artifacts
  line ceiling, kit anti-leakage inventory, preserved-channel shell handling,
  Homebrew recovery ref, and Homebrew bundled-path rendering); this count is
  independently re-verified rather than copied from the superseded dispatch.

## Constraints

- no Rust source changes; the bump recipe MAY edit version fields in crates/*/pyproject.toml and the workspace Cargo.toml/Cargo.lock, and nothing else under crates/
- No sc-publish kit files are hand-edited.
- The prerelease workflow is tag-triggered only and has no publishing token or
  write permission.
- The direct bump commit is intentionally pushed to the operator's current
  branch; no PR is created by the recipe.

## QA history

### QA-1 — 2026-08-29

QA-1 identified one active-plan ambiguity, one stale preflight link, and three
Windows shell-fixture reliability gaps. Every QA-1 finding and its closing
commit on `feature/ar1-prerelease-archive` is recorded below:

- `ATM-QA-001` — `3c55f6a46` distinguishes the active AS1.1
  release-validation scope from the superseded Phase AS migration.
- `ARCH-001` — `915aa38c9` links release preflight to this authoritative
  AS1.1 plan.
- `RBQA-F001` — `c03c97137` centralizes POSIX/Git-Bash discovery and rejects
  the System32 WSL launcher.
- `FTQ-001` — `7d6d149b1` proves prerelease shell discovery ignores ambient
  `GIT_BASH`.
- `FTQ-002` — `57e7f50e2` bounds resolver and extracted-workflow shell
  commands.
- `ATM-QA-002` — `11d351cfd` adds this complete QA-1 history.
- `ATM-QA-003` — `c460ad8c5` documents Git-Bash versus the System32 WSL
  launcher for test authors.

The remaining documentation clarifications are tracked as QA-1 follow-ups in
this plan and the cross-platform guidelines; they do not change the release
workflow or its archive contract.

### QA-2 — 2026-08-29

Windows CI found the original shared-resolver unit tests were unintentionally
observing a real Git-for-Windows installation instead of their mocked PATH.
`e23dac88a` fixes that test isolation, gives the picker subprocess helper the
same finite timeout policy, and records the complete seven-finding QA-1 map
above. Production Git-Bash discovery remains enabled on Windows.
