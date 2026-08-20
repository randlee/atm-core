# AT.1 — Canonical Consumer Install And Preflight Parity

```yaml
plan_type: sprint_plan
phase: AT
sprint: AT.1
worktree: feature/pat-s1-canonical-consumer-install
branch: feature/pat-s1-canonical-consumer-install
status: proposed
estimated_scope: shared-package install plus ATM-owned manifest input only
```

## Goal

Install one immutable `sc-publish` package revision into an ATM worktree with
complete explicit JSON and prove that the resulting preflight and release
paths use identical shared assets and tool bootstrap.

## Deliverables

1. Record the selected immutable `sc-publish` commit and package digest in the
   ATM consumer input/evidence. The pin must resolve upstream issues #39–#41
   (see phase README); `ce85b4d` is the floor, not the pin. Run the packaged
   installer; do not copy, modify, or regenerate individual shared files by
   hand.
2. Author the complete ATM-owned `release/sc-publish-consumer-input.json` for
   all release crates, Python distributions, binaries, channels, publish
   order, and version source. Rendered manifests must name every declared
   distribution and support Maturin and setuptools builds through their
   declared build system. A schema-validated draft exists from the 2026-08-20
   reconciliation spike (the AS-era input needed one addition — `test_binary`
   on each Homebrew formula — to satisfy the `ce85b4d` schema); the sprint
   re-derives and reviews the full document rather than trusting the draft.
3. Use the package's single tool bootstrap in both preflight and release.
   Remove no legacy workflow until AT.3's coverage gate; where the installer
   overwrites a same-named legacy file (`release.yml`,
   `release-preflight.yml`), the overwrite is the adoption.
4. Lock all released crates and Python deliverables to the workspace version.
   Python metadata may derive its version dynamically only from the same
   workspace version; wheel filenames remain valid PEP 440 versions and never
   use an ATM `-beta` suffix.

The consumer input must express Python build ownership explicitly; the shared
workflow chooses its command from this contract rather than guessing from a
package name or a null path:

```json
{
  "python_distributions": [
    {
      "name": "native-package",
      "build_system": "maturin",
      "cargo_manifest": "crates/native-package/Cargo.toml",
      "sdist": true,
      "wheels": ["ubuntu-latest", "macos-latest", "windows-latest"]
    },
    {
      "name": "pure-python-package",
      "build_system": "setuptools",
      "sdist": true,
      "wheels": ["ubuntu-latest", "macos-latest", "windows-latest"]
    }
  ]
}
```

`maturin` is valid only with a `cargo_manifest`; the shared workflow builds
the setuptools form with its declared Python build backend. A missing or
unsupported `build_system` is a manifest validation failure. `hermes-atm`
(setuptools) and the Maturin-built native bindings must both be declared and
both build through this contract.

## Prerequisites

- A clean ATM worktree off `develop` (baseline `d610b4c07`, post-PR #960) and
  an immutable, locally available `sc-publish` revision resolving #39–#41.
  Network unavailability may delay selecting a newer revision, but does not
  justify a local fork or hand edit.
- The caller has reviewed the complete consumer JSON before installation.

## Dependencies

- `AT.2`: `must_follow`; real publication consumes only the exact installed
  package/input/manifest tuple proven here.
- No other phase dependency. This sprint is parallel-safe with product work
  because it changes only publishing assets and release metadata.

## Non-Goals

- No product, daemon, Hermes, or crate behavior change.
- No production or TestPyPI dispatch.
- No copied shared-file modification, local publishing framework, or
  repo-specific branch in `sc-publish`.

## Paths To Delete

None. Existing release assets remain until AT.2 proves the installed shared
path covers their retained behavior; AT.3 owns the explicit deletion decision.

## Acceptance Criteria

- `install.py --dry-run` reports no drift after installation.
- Every copied shared file byte-compares equal to its package source; the only
  consumer-specific outputs are the installer-designated manifests.
- The complete JSON and parsed manifests agree on publish surface, explicit
  order, all Python distributions, and each declared build system.
- A fixture containing both the `maturin` and `setuptools` forms proves the
  shared workflow selects the declared backend and rejects an absent or
  unsupported build system before any upload.
- Preflight and release resolve the same package revision, manifest digest,
  and tool bootstrap; tests fail if either path selects a different source.
- Version-sync validation proves all released crates and Python distributions
  resolve to one workspace version.

## Required Validation

```bash
<bootstrap-python> plugins/sc-publish/install.py --dry-run --input release/sc-publish-consumer-input.json <worktree>
diff -ru --exclude publish-artifacts.toml --exclude publish-channel-contracts.toml <sc-publish-package-root>/release <worktree>/release
python3 .github/scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
python3 .just/check_version_sync.py
```

Run the package's own installer/unit-test suite for the mixed-build-system
fixture. ATM does not duplicate that shared test suite.

## Required Document Updates

- Record the immutable `sc-publish` revision, package digest, complete
  consumer-input digest, and generated-manifest digests in the AT.1 receipt,
  and update the phase README pin.
- Record any shared-package defect as an upstream `sc-publish` issue/PR with
  the failing fixture and command; do not patch its copied ATM files
  (ADR-050).

## Risks And Watchouts

- A package or installer defect is reported and fixed in `sc-publish`; AT.1
  must then repeat the install and proof against the new immutable revision.
- Upstream moves fast; adopting a newer revision restarts AT.1's proof and is
  never a reason to hand-merge changes.
- A credential result is not a package-install failure and is not grounds for
  inventing a local credential workaround.
