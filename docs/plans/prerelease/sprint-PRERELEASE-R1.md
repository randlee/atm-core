# Sprint PRERELEASE-R1 — `/prerelease --publish` and `/prerelease --install`, GitHub-only, sc-publish native

Spec: atm-core issue #1350 (read it first, including the comments). Rand's rulings: prerelease binaries are published to **GitHub only** (a `prerelease/vX.Y.Z` GitHub *prerelease* Release carrying every manifest-declared archive and `checksums.txt`); Homebrew, crates.io, PyPI/TestPyPI, winget and Scoop are never touched by this path. The capability lives in the **sc-publish kit** so every kit repo gets it from its manifest alone; atm-core is the first adopter.

## Deliverables

### A. sc-publish kit (repo `randlee/sc-publish`, worktree under `~/Documents/github/sc-publish-worktrees/feature/prerelease-skill`, branch `feature/prerelease-skill`, PR into that repo's default branch)

1. `plugins/sc-publish/.github/workflows/prerelease-archive.yml` (template): triggered by `<tag_prefix-prerelease>vX.Y.Z` tags (default `prerelease/v`), verifies tag == workspace version, builds the manifest `release_binaries` for the manifest target matrix, packages the same archive names `release.yml` produces, generates `checksums.txt`, then **creates or updates a GitHub Release for that tag with `--prerelease`** and uploads the archives + `checksums.txt` as assets. Atm-core's existing `.github/workflows/prerelease-archive.yml` is the reference implementation minus the Release step; keep its verify-version and lockstep checks.
2. `plugins/sc-publish/.claude/skills/prerelease/SKILL.md` + `scripts/prerelease.py` (stdlib only):
   - `--publish X.Y.Z` (or `--bump` from a clean non-protected branch): tag via the repo's prerelease tag script (atm-core: `.just/prerelease_tag.py`; manifest key names it), wait for the prerelease-archive run for that tag to succeed, verify the Release exists with every expected asset and that `checksums.txt` matches the downloaded archives, print the Release URL. Refuses on a protected branch, dirty tree, or missing `gh` auth. Runs only on explicit operator authorization (the skill text says so).
   - `--install [X.Y.Z]` (default: newest `prerelease/v*` prerelease Release): download the host-triple archive from `releases/download/`, verify sha256 against `checksums.txt`, stage under the manifest `install_root/vX.Y.Z/`, repoint the PATH selector symlinks the manifest names (per-OS table; Windows = copy into the selector dir), run the manifest `post_install` command with `{version}` and `{stage_dir}` substituted, then run `verify` and fail unless its output contains `X.Y.Z`. Idempotent; re-running on the installed version is a no-op that still verifies.
   - Manifest block in `release/publish-artifacts.toml.j2`:
     ```toml
     [prerelease]
     tag_prefix = "prerelease/v"
     tag_script = ".just/prerelease_tag.py"
     install_root = "~/.atm-builds"
     binaries = ["atm", "atm-daemon"]
     selector_dir = { darwin = "/opt/homebrew/bin", linux = "~/.local/bin", windows = "%LOCALAPPDATA%\\Programs\\ATM" }
     post_install = "python3 .claude/skills/daemon-switch/scripts/daemon-switch.py switch --prerelease {version} --yes"
     verify = "atm --version"
     ```
     Validate it in `release_artifacts.py validate-manifest` (missing block = feature off, not an error).
3. Kit docs: README section + skill text; the `install.py` adopter path installs the new skill and workflow like the existing ones.
4. Tests: unit tests for manifest parsing, asset-name derivation, checksum verification, selector repointing (temp dirs), `--install` no-op path; a workflow lint (`actionlint` if present in the kit's CI, else the kit's existing yaml check).

### B. atm-core adopter (this worktree, branch `feature/prerelease-skill`, PR into `develop`)

1. `release/publish-artifacts.toml`: add the `[prerelease]` block above.
2. `.github/workflows/prerelease-archive.yml`: regenerate from / align with the kit template so the Release step exists (keep the artifact uploads the colima testbed consumes — `test.sh` downloads `aarch64-unknown-linux-gnu` and CI wheels by run id; do not break that).
3. `daemon-switch switch --prerelease <X.Y.Z|latest>`: new mode in `.claude/skills/daemon-switch/scripts/daemon-switch.py` + `release_resolution.py`: resolve via the GitHub Releases API allowing `prerelease: true` for tags with the prerelease prefix (stable `--release` behaviour unchanged), download + checksum-verify the host archive if not already staged, stage under `~/.atm-builds/vX.Y.Z/`, on macOS sign the pair with the configured identity exactly as the `--worktree` path does (`python3 .just/sign_daemon_dev.py` equivalent), then switch + restart + prove the managed pair as the other modes do. Homebrew formula untouched; `homebrew_restore_available` stays true. Update `SKILL.md` (modes table, example) and the Python tests beside the scripts.
4. `docs/release-preflight-checklist.md` §"Prerelease Archives": document the Release step and the two skill commands; `.claude/skills/publishing/SKILL.md` gets one cross-reference line (prerelease ≠ release).

## Acceptance

- Kit PR: `prerelease.py --help` documents both modes; tests pass; a dry run (`--publish --dry-run`) on the kit's own fixture manifest prints the plan without network calls.
- atm-core PR: `python3 .claude/skills/daemon-switch/scripts/daemon-switch.py switch --prerelease 1.5.11 --yes --service <label> --launch-agent-plist <plist>` on rand-m5 ends with `atm --version` == 1.5.11 on PATH, `status --doctor` healthy, daemon restarted, Homebrew formula unchanged. Use the existing prerelease/v1.5.11 artifacts: do NOT create a Release for 1.5.11 or dispatch any workflow; a Release for testing is created only when Rand authorizes it in writing (he has not). Until then, unit-test the download/verify path against local fixture archives.
- Both PRs: no unused code, no edits to lint config/allowlists/boundaries, QA report on the PR before merge.

## Out of scope

Production `release.yml` and all non-GitHub channels; the colima testbed (`test.sh` keeps consuming Actions artifacts until a follow-up moves it to Release assets).
