# Sprint AR1.1 — Pre-release archive job (linux/macOS/windows binaries from any ref)

**Branch**: `feature/ar1-prerelease-archive` → PR targets `develop`
**Worktree**: `../atm-core-worktrees/feature/ar1-prerelease-archive`
**Owner**: fenix (Phase AR) · **Consumer**: loki@hermes docker-testbed
(hendrix/hendrix/loki/docker-testbed) and the Mac daemon-switch install for
cross-host parity.

## Problem

`atm_<ver>_<target>.tar.gz|zip` archives are produced only by `release.yml`
(`workflow_dispatch`, version must be `X.Y.Z`, creates the tag). Phase AR
integration testing needs 1.4.5 **pre-release** binaries for all manifest
targets, built from an arbitrary ref (develop head), CI-provenanced, with the
same layout the installer path expects — without tagging or publishing
anything.

## Constraint this sprint discovered: `release.yml` and `release_artifacts.py` are vendored sc-publish kit files

The original brief for this sprint asked for a new `package-archive`
subcommand in `.github/scripts/release_artifacts.py` and for `release.yml`'s
inline packaging step to be replaced with a call to it. Both files are
governed by the sc-publish publish kit (see `README.sc-publish.md`, the pin
at `release/sc-publish-pin.toml`, and
`.github/scripts/tests/test_release_artifacts.py::test_no_single_repo_concerns_leak_into_kit_workflows_actions_or_scripts`,
which enumerates `release.yml` and `release_artifacts.py` as pinned,
byte-for-byte-installed kit files): "Every kit file is installed byte-for-byte
into the consumer repository — copied files are never hand-edited... local
drift is a defect, not a customization mechanism." This is also the
project's standing sc-publish governance model: shared release-kit changes
land as PRs against `randlee/sc-publish`, reviewed jointly, then adopted here
by re-pinning — never as unilateral local edits in `atm-core`.

**Resolution taken this sprint:** `release.yml` and `release_artifacts.py`
were left untouched (no `package-archive` subcommand added; `release.yml`'s
packaging step is unmodified). The new workflow,
`.github/workflows/prerelease-archive.yml`, is instead an atm-core-owned
addition — the kit's anti-leakage inventory test explicitly allows consumers
to add their own workflows — and its packaging step **duplicates**
`release.yml`'s "Package manifest-declared release archive" step verbatim
(only the version-source expression differs: `needs.gate-and-tag.outputs.release_version`
vs. `needs.plan.outputs.version`).
`test_prerelease_archive_packaging_matches_release_yml_byte_for_byte` (in the
new, also atm-core-owned, `.github/scripts/tests/test_prerelease_archive_workflow.py`)
asserts the two scripts are identical modulo that one line, so the
duplication cannot silently drift.

Consolidating the two packaging steps behind one shared `package-archive`
subcommand remains the better long-term shape, but that change belongs in a
PR against `randlee/sc-publish`, reviewed the way every other kit change is,
then adopted here by advancing `release/sc-publish-pin.toml`. Filing that as
a follow-up (`randlee/sc-publish` issue) is recommended, not done as part of
this sprint.

## Delivered

1. **`.github/workflows/prerelease-archive.yml`** (new, atm-core-owned, not a
   kit file):
   - `on: workflow_dispatch` with inputs `ref` (default `develop`) and
     optional `targets` (comma-separated target triples).
   - `plan` job: resolves the exact commit SHA for `ref`, computes the
     pre-release version `<workspace version>-pre.<9-char short sha>` (e.g.
     `1.4.5-pre.eab52792e`), asserts it is not tag-style (`X.Y.Z`), reads
     `rust_toolchain` from the manifest, and resolves the requested release
     target matrix from `release-target-matrix` — **unmodified** vendored
     subcommands only.
   - `build` job: one job per resolved target, built with
     `cargo-build-bin-args` (existing subcommand, unmodified), packaged with
     the duplicated-but-pinned-equivalent inline step described above, and
     uploaded as an artifact named `<target>`.
   - `checksums` job: downloads every target artifact, writes `checksums.txt`
     (`<sha256>  <filename>`, the same two-space format `release.yml`'s
     `sha256sum` output and `shasum -a 256 -c` produce/verify) and
     `provenance.json` (`{version, atm_core_sha, ref, run_id, targets:
     [{target, archive, sha256}]}`), uploaded as one `checksums` artifact.
   - No tagging, no GitHub Release, no publishing, no secrets.
     `permissions: contents: read`. Concurrency group keyed on `ref`.
     `timeout-minutes` set on every job.
   - **Default targets: all four manifest `[[release_targets]]` entries**
     (`x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`,
     `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`) — macOS is not opt-in.
     The Mac daemon-switch parity install needs `aarch64-apple-darwin` from
     the same run as the Linux drop, so narrowing is the operator's
     responsibility: pass `targets=x86_64-unknown-linux-gnu` (or any other
     comma-separated subset) when only one target's binaries are needed.
     Because the matrix is entirely manifest-derived
     (`release-target-matrix`), a future release target added to
     `release/publish-artifacts.toml` (for example a native
     `aarch64-unknown-linux-gnu` target) flows into this job automatically,
     with no workflow change required.
2. **`.github/scripts/tests/test_prerelease_archive_workflow.py`** (new,
   atm-core-owned test file, deliberately separate from the vendored
   `test_release_artifacts.py`): asserts the packaging-step parity with
   `release.yml` described above, executes the extracted packaging script
   against a fixture tree (tar.gz/zip layout, Windows `.exe` suffix), and
   checks the workflow never tags/publishes/reads secrets, defaults to every
   manifest target, rejects a tag-style version, and writes
   checksums/provenance in the documented shape.
3. **Docs**: this document, and an operator section in
   `docs/release-preflight-checklist.md` describing
   `gh workflow run prerelease-archive.yml -f ref=<sha> [-f targets=...]` and
   `gh run download <run-id> -n x86_64-unknown-linux-gnu`.
4. **Not delivered from the original brief, and why:** no `package-archive`
   subcommand in `release_artifacts.py`; no edit to `release.yml`'s inline
   packaging step; no edit to the vendored
   `.github/scripts/tests/test_release_artifacts.py`. See the constraint
   section above.

## Verification

- `pytest .github/scripts/tests/test_prerelease_archive_workflow.py -q` —
  green.
- `pytest .github/scripts/tests/test_release_artifacts.py -q` — run as a
  regression check only (this sprint made no edits inside that file or its
  vendored subjects). Baseline on `origin/develop @ eab52792e`, before any
  change in this worktree, already had 5 failing tests unrelated to this
  sprint (line-ceiling drift on `release_artifacts.py`, the anti-leakage
  inventory test, a homebrew-formula Ruby-rendering assertion, and two
  channel-recovery assertions) — see the PR for the exact list. This sprint
  neither introduces new failures nor fixes the pre-existing ones; fixing
  them requires either editing pinned kit files (out of scope, see above) or
  advancing the sc-publish pin.
- `just lint` — green (the vendored kit's own test suite is not part of
  `just lint`'s `pytests` target, which only covers `.just/tests`,
  `scripts/smoke`, and `scripts/phase-aq`).

## Constraints (from the original brief, still honored)

- No `crates/` changes; no daemon code. CI-yml + `.github/scripts` + docs
  only.
- Windows CI compliance per `docs/cross-platform-guidelines.md`.
- Commit messages: `feat(ci): ...`, `docs(ar): ...`; never squash; single
  push at the end.
- The new workflow is not dispatched and CI is not polled as part of this
  sprint; the PR reports the pushed head and stops there.
