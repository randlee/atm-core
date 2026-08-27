# Phase AT sc-publish Re-pin Verification Receipt

```yaml
receipt_type: verification
branch: smoke/phase-at-repin-verify
consumer_base: origin/develop @ 5c8127c56aebd11b3c19fd23a9601367f0f5ce33
sc_publish_revision: 42e0fcea23f730fae0ef3d08b060cd4df6a2602e
verdict: compatible
```

## Inputs and package identity

| Item | Value |
| --- | --- |
| Recovered consumer input | `release/sc-publish-consumer-input.json` |
| Consumer-input SHA-256 | `7fbcb24b6afbacbc4d0259527cf851e255809fa1fbda8ae9233a13dcde24643b` |
| New package digest | `75da9d18426eacb92bab3e02bb6655a35e14f69deafe1ba2d00f4e93aabc0a5b` |
| Installable package files | 49 |
| Package digest algorithm | SHA-256 over the newline-joined, path-sorted `"<sha256(file)>  <relative-path>"` lines from `install.package_files()` |

The input was recovered byte-for-byte from
`origin/smoke/phase-at-at1-rehearsal:release/sc-publish-consumer-input.json`.
Its digest matches the expected rehearsal digest above. The new package digest
and count replace the old-pin values `a0da19677d150e565e724d99a442f660974520d4f429b71da1b3f03a6bbc1a83`
and 48 respectively.

The local checkout at `/Users/randlee/Documents/github/sc-publish` resolved
`develop` to the requested SHA but had an untracked `.sc-compose/` directory.
It was not changed. Verification instead used a fresh, clean, detached clone
at `/tmp/sc-publish-repin-42e0fce` at the same SHA.

## Installer and parity validation

All commands ran from this scratch consumer worktree and invoked the installer
from the fresh upstream clone, never the installed consumer-root `install.py`.

| Command | Exit | Result |
| --- | ---: | --- |
| `python3 $SP/plugins/sc-publish/.github/scripts/bootstrap_sc_compose.py --venv .venv-sc-publish` | 0 | Installed prebuilt `sc-compose==1.4.1`; no cargo build/install was used. |
| `./.venv-sc-publish/bin/python $SP/plugins/sc-publish/install.py --input release/sc-publish-consumer-input.json .` | 0 | Installed the 49 package files and rendered the two consumer manifests. |
| `./.venv-sc-publish/bin/python $SP/plugins/sc-publish/install.py --dry-run --input release/sc-publish-consumer-input.json .` | 0 | `Publish-kit assets are in sync.` |
| Package-file byte parity sweep using `install.package_files()` and `RENAMED_FILES` | 0 | `byte-identical: 49, mismatched/missing: 0` |
| `./.venv-sc-publish/bin/python -m pytest -q .github/scripts/tests/` | 0 | `81 passed, 8 skipped, 3 subtests passed in 5.67s` |
| `python3 .github/scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml` | 0 | `manifest validation passed` |

The bootstrap venv intentionally provides only `sc-compose`; the first pytest
attempt reported `No module named pytest`. This is a non-blocking missing test
dependency, not an installer/schema failure. `pytest==9.1.1` and its normal
dependencies were installed only into the disposable `.venv-sc-publish`, then
the required packaged suite passed as recorded above.

Rendered manifest SHA-256 values:

| File | SHA-256 |
| --- | --- |
| `release/publish-artifacts.toml` | `f04d042c988be74e7386a5fe191b95e44a679456b9718810341dd602716a2407` |
| `release/publish-channel-contracts.toml` | `88d7ac055689c1e9fcd752c178eac9094ca29084a097fa7aff109c4fb0f40fd8` |

## Release-candidate provenance and PyPI retry

The #48 release-candidate provenance gate does **not** add a precondition to
the `pypi-publish.yml` `workflow_dispatch` interface. That workflow exposes
only required `tag` and `target` inputs (lines 8-22), checks out
`ref: ${{ inputs.tag }}` and invokes `verify-published-release` with that tag
(lines 38-43). It then downloads only `*.whl` and `*.tar.gz` from that
published GitHub Release (lines 63-81).

The candidate gate is enforced earlier in the release path:

- `release-candidate.yml` constructs
  `candidate_tag="release-candidate-${release_tag}"` and, when reusing it,
  requires `git merge-base --is-ancestor "${candidate_tag}" origin/develop`
  (lines 47-60).
- `release.yml` invokes
  `.github/scripts/release_gate.sh final origin/main "release-candidate-${{ steps.meta.outputs.release_tag }}" ...`
  before the release tag work (lines 93-95).

The PyPI retry path is therefore intentionally post-release: it does not
re-run the release-candidate gate, but it does require a confirmed non-draft
published GitHub Release with every required asset. The shared
`verify-published-release` action fails when the release is absent, draft, or
missing a required asset (lines 69-110). Thus a retry for immutable `v1.4.3`
with assets already attached satisfies the PyPI workflow's gate and bypasses
only a redundant re-evaluation of the earlier provenance gate; it neither
creates a tag nor rebuilds assets.

## Non-blocking outcomes and scope

No GENUINE STOP outcome occurred: there was no immutable-asset hash mismatch
and no public-index version discrepancy. The temporary missing pytest module
was resolved in the disposable verification venv. No consumer kit file was
hand-edited, no product code was changed, and generated kit files are left
uncommitted in this disposable worktree; the committed evidence is this
receipt and the recovered input only.
