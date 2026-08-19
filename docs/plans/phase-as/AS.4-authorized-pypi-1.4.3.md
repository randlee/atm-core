# AS.4 — Authorized PyPI 1.4.4 publication

```yaml
plan_type: sprint_plan
phase: AS
sprint: AS.4
worktree: main immutable release assets; canonical publisher agents from AS worktree
branch: main
status: blocked
estimated_scope: one authorized production channel
```

## Goal

**Amended 2026-08-18** (Rand, direct decision): the already-built immutable
`1.4.3` artifacts predate both the ADR-049 first-public-release disclosure
and the current `[[python_distributions]]` manifest contract, and AS.4's
original Non-Goals forbid rebuilding/retagging `1.4.3` to fix that. Rather
than fold this into AS.6, AS.4 now cuts and publishes a fresh, minimal
version — `1.4.4`, PyPI channel only — with the disclosure and current
manifest baked in from the start. This keeps AS.4 as the narrow,
low-risk proof that the PyPI channel actually works end-to-end, ahead of
AS.6's larger multi-channel release. Publish only the newly built `1.4.4`
Python artifacts through the canonical PyPI channel and retain a public
verification receipt. AS.6 remains the full multi-channel release; it may
target a later version, or complete the remaining (non-PyPI) channels for
`1.4.4` if versions line up — that call is made when AS.6 is dispatched, not
here.

## Scope Summary

This is an authorized production operation. Unlike the original AS.4 design,
it does include a version bump and a fresh build/tag — narrowly scoped to
carrying the ADR-049 disclosure and the current manifest contract, PyPI
channel only. It is still not a full multi-channel release; that remains
AS.6's job. The canonical agents may be launched from the Phase AS branch.

## Governing Requirements

- Explicit production authorization is mandatory.
- Publication uses attached immutable assets and the AS.3-matched receipt.
- Python artifact verification covers 3.11, 3.12, 3.13, and 3.14.

## Governing ADRs

- [ADR-049](../../adr/ADR-049-hermes-atm-first-public-pypi-release-versioning.md)
  governs the first-public-release disclosure for `hermes-atm` and `atm-graft`.

## Governing Boundaries

- Only PyPI is enabled. Crates, tag, GitHub Release, Homebrew, Scoop, winget,
  and other channels are excluded.

## Prerequisites

- **Amended 2026-08-18** (see phase [README](README.md) governing-boundaries
  amendment): AS.3 evidence is atm-core's own preflight validation, run via
  the workspace-source `sc-lint-boundary` crate directly rather than gated on
  the externally-released `sc-lint` binary's schema catching up (sc-lint#115)
  or on `sc-publish`'s upstream fail-closed receipt PR (sc-publish#25)
  merging first. Neither upstream PR is a blocking dependency for this
  sprint.
- A freshly built, tagged `1.4.4` on immutable `main` with the ADR-049
  disclosure and current manifest contract, and manifest-matching artifacts.
- Human production authorization.
- ADR-049’s first-public-release disclosure is present in the package README
  and GitHub release notes before the PyPI action — an atm-core-owned doc fix,
  not blocked on anything upstream.

## Hard Dependencies

- `AS.3`: `must_follow`; receipt digest must match an atm-core-owned
  validation receipt (not an external `sc-publish` receipt PR).
- `AS.5`: `must_follow`; migration merge waits for publication proof.

## Non-Goals

- Publishing crates, GitHub Release binaries, Homebrew, Scoop, winget, or any
  channel other than PyPI.
- Rebuilding or republishing the now-superseded `1.4.3` immutable artifacts;
  they stay as-is, unpublished, superseded by `1.4.4`.
- Any version bump or unrelated change beyond the single `1.4.4` bump needed
  to carry the ADR-049 disclosure and current manifest contract.

## Sub-Tasks

1. Verify artifact checksums, version, and main commit against the approved
   manifest and AS.3 receipt; on any receipt mismatch, block publication,
   record escalation, and require a fresh matching preflight after upstream
   correction.
2. Execute only the canonical PyPI channel.
3. Query the public package index and install/verify each supported Python
   version from published artifacts.
4. Store channel and public verification receipts.

## Split Recommendation

Keep this one-channel `1.4.4` PyPI release separate from AS.6, which remains
the full multi-channel release (a later version, or the remaining non-PyPI
channels for `1.4.4` — decided when AS.6 is dispatched).

## Acceptance Criteria

- Only the authorized PyPI channel ran.
- Public PyPI exposes the approved 1.4.4 artifacts.
- Python 3.11–3.14 verification succeeds.
- Receipts prove immutable source/artifact identity and public availability.
- The ADR-049 first-public-release disclosure is visible in the published
  package README and release notes.

## Required Validation

```bash
python3 .github/scripts/release_artifacts.py verify-python-release-assets \
  --manifest release/publish-artifacts.toml --asset-dir dist
python3 -m pip install --only-binary=:all: hermes-atm==1.4.4
```

## Required Document Updates

- Record authorization, workflow run, artifact digests, index URLs, and Python
  matrix evidence.

## Risks And Watchouts

Existing target versions must be classified with PF-3 semantics; do not treat
an already-published version as permission to rebuild or overwrite it.

## Execution Receipt — 2026-08-18

**Outcome: blocked before production upload.** The AS.4 forward branch is
`feature/pas-s4-authorized-pypi-1.4.3`, stacked from AS.3 evidence commit
`3f5f2db474dc66b34be38d60193ba259fe8e78e4`. The immutable release source is
`main`/`v1.4.3` commit `fa6066468f8e638893bedd686244cffa2a43dbdf`.

Release Preflight run
[`32159872873`](https://github.com/randlee/atm-core/actions/runs/32159872873)
failed because external `sc-lint` 0.4.0 rejected ATM's legitimate `trait`
boundary field. This was superseded locally in AS.4: preflight now runs
ATM's workspace-source `sc-lint-boundary` analyzer directly, and the local
PyPI path writes a source-commit, manifest-SHA-256, version, and per-artifact
SHA-256 receipt before upload. No upstream `sc-lint` or `sc-publish` merge is
required for those checks.

**Boundary-lint triage (2026-08-18):** The workspace analyzer reports
`status: fail` with 21 findings on both the AS.3 baseline commit
`3f5f2db474dc66b34be38d60193ba259fe8e78e4` and this AS.4 branch: seven
`SCB-CYCLE-001` multi-owner architectural cycles, nine `SCB-CYCLE-002`
type/method self-loops, and five `SCB-CYCLE-003` trait-implementation
self-loops. The seven multi-owner cycles are pre-existing hard failures under
the analyzer's `finding_is_failure` policy; the self-loop categories are
reported advisory findings. They are neither introduced by AS.4 nor accepted
as clean, and AS.4 does not suppress or reclassify them. The preflight gate
therefore captures the analyzer JSON and exits nonzero unless its status is
`pass`, while retaining `continue-on-error` so the final receipt records the
real boundary-lint failure rather than aborting all remaining evidence.

The immutable tag also fails ADR-049's publication-disclosure prerequisite:
neither `crates/hermes-atm/README.md` nor the v1.4.3 GitHub release notes
states that this is the first public release and explains the internal 1.x
history. That cannot be corrected without a new immutable release; AS.4
therefore must not upload these artifacts.

At the time of this receipt, both public registry endpoints returned 404 for
`hermes-atm` and `atm-graft`; no public PyPI artifacts exist. The prior
workflow run [`32081875532`](https://github.com/randlee/atm-core/actions/runs/32081875532)
successfully uploaded only to TestPyPI; its production-PyPI job was skipped.
Its immutable staged artifact SHA-256 values were:

- `hermes_atm-1.4.3-py3-none-any.whl` —
  `041311b7806d43acd2326c6532076b614f6df9af930f6102d4c8485aa936b2f7`
- `atm_graft-1.4.3.tar.gz` —
  `c9d6f532d6606f9f1cbc90e8970e7817ce1444dc75e44906ed4ad5b688f880ab`
- `atm_graft-1.4.3-cp311-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` —
  `3344cabb6c1f0b1c02e9df0174b28dd72577021e92d784fc307dae693adfc26b`
- `atm_graft-1.4.3-cp311-abi3-manylinux_2_17_aarch64.manylinux2014_aarch64.whl` —
  `11aa4d7059a94854d8acec11df2200359e660226176907397d3961464f0d7337`
- `atm_graft-1.4.3-cp311-abi3-musllinux_1_2_x86_64.whl` —
  `32b713a0cf4508dbffe2a865194c471e1836ec6d7a744938c1be7e7a64f9e83e`
- `atm_graft-1.4.3-cp311-abi3-macosx_11_0_arm64.whl` —
  `23da2aa35fef045c9899063d00495a384506d0ff995d4728d378962fb3b9aeac`
- `atm_graft-1.4.3-cp311-abi3-win_amd64.whl` —
  `f9963df87f5d4e04377583200e79227b8c1db9f22a507b37bff3de43c6e2112a`

The immutable v1.4.3 release manifest also predates the current
`[[python_distributions]]` contract, so it cannot produce a contract-matching
receipt through the current PyPI workflow. Required remediation is therefore
a new immutable release containing the ADR-049 disclosure and current release
manifest, followed by the local AS.3 validation receipt and the PyPI-only
channel. No rebuild or upload is authorized for the blocked v1.4.3 set.

### 1.4.4 preflight re-runs

The fresh `1.4.4` cut was prepared in `a22168eec` and the preflight workspace
test bootstrap was added in `df54f34f7`. Release-preflight run
[`32169019885`](https://github.com/randlee/atm-core/actions/runs/32169019885)
was the first `1.4.4` attempt and exposed that `compose_passthrough` needs the
`sc-compose` executable, not only Python bindings. Run
[`32169985864`](https://github.com/randlee/atm-core/actions/runs/32169985864)
re-ran after that pinned CLI bootstrap and passed the workspace checks; its
remaining channel-only failure was the rejected `CARGO_REGISTRY_TOKEN` for
crates.io. That credential decision remains pending Rand and does not
authorize a PyPI upload.

### Draft v1.4.3 GitHub release note

> **First public PyPI release:** `hermes-atm` and `atm-graft` begin public
> distribution at ATM's existing 1.x workspace version. Earlier 1.x
> development was internal, not a missing public release history; the Python
> packages remain version-locked to the ATM workspace release.
