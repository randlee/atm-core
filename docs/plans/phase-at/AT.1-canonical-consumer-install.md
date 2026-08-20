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

1. Record the pinned immutable `sc-publish` commit
   (`0fa5b05e44a655ec76ada8a6c2b24714d47acca1`, see phase README) and package
   digest (algorithm per the phase README Receipt Convention) in the ATM
   consumer input/evidence. Run the packaged installer and dry-run FROM the
   pinned checkout — `<sc-publish-checkout>/plugins/sc-publish/install.py` —
   never the vendored copy the installer places at the consumer repo root
   (its `PACKAGE_ROOT` resolves to the consumer repo; running it is
   destructive — upstream issue sc-publish#46). Do not copy, modify, or
   regenerate individual shared files by hand.
2. Author the complete ATM-owned `release/sc-publish-consumer-input.json` for
   all release crates, Python distributions, binaries, channels, publish
   order, and version source. Every crate participating in a release
   artifact must be declared, **including `publish = false` crates that feed
   a released artifact** (e.g. `atm-graft-python` feeding the `atm-graft`
   wheel, and symmetrically `atm-query-python`), per the existing manifest
   convention — the Ground Truth internal-crates table below is the
   reference. Rendered manifests must name every declared distribution and
   support Maturin and setuptools builds through their declared build
   system. The authoritative input schema is `load_install_values` in
   `plugins/sc-publish/install.py` at the pinned revision; a complete worked
   example exists at
   `smoke/phase-at-at1-rehearsal:release/sc-publish-consumer-input.json`.
   Rehearsal evidence exists on branch `smoke/phase-at-at1-rehearsal` (that
   consumer input plus its receipt at
   `docs/plans/phase-at/AT.1-rehearsal-receipt.md` on that branch); it is
   evidence and reference only, not authority — the sprint re-derives and
   reviews the full document against the Publish Surface Ground Truth below
   rather than trusting the rehearsal draft.
3. Use the package's single tool bootstrap in both preflight and release.
   Remove no legacy workflow until AT.3's coverage gate; where the installer
   overwrites a same-named legacy file, the overwrite is the adoption. The
   complete expected-overwrite list at the pinned revision (per rehearsal):
   `.github/workflows/release.yml`, `.github/workflows/release-preflight.yml`,
   `.claude/agents/publisher.md`, and `release/publish-artifacts.toml`. The
   AT.1 receipt lists **every** file the installer created or overwrote (48
   files at the pin), including the installed agent files.
4. Lock all released crates and Python deliverables to the workspace version.
   Python metadata may derive its version dynamically only from the same
   workspace version; wheel filenames remain valid PEP 440 versions and never
   use an ATM `-beta` suffix. The enforcement mechanism is the consumer
   input's `project.workspace_toml` declaration (`Cargo.toml`, i.e.
   `[workspace.package].version`): it is the single version source all
   crates and distributions must resolve to, verified by
   `.just/check_version_sync.py`.
5. Enumerate the per-channel GitHub secrets and environments the kit
   requires, from the vendored `release/publish-channel-contracts.toml` at
   the pinned revision (authoritative list; fixed cross-repo names):
   repository secrets `CARGO_REGISTRY_TOKEN`, `HOMEBREW_TAP_TOKEN`,
   `WINGET_GITHUB_TOKEN`, `SCOOP_BUCKET_TOKEN`; environment secrets
   `PYPI_API_TOKEN` (environment `pypi`) and `TEST_PYPI_API_TOKEN`
   (environment `testpypi`); environments `crates-io`, `pypi`, `testpypi`.
   Record each one's configured/missing state in the AT.1 receipt as
   fail-closed evidence, satisfying REQ-P-RELEASE-006. A missing entry is
   evidence, not a blocker (phase README Non-Blocking Outcomes table).
6. Enumerate the installed kit workflows' job/check names against the
   branch-protection required checks configured on `integrate/phase-at` and
   `develop`; update the protection settings (or record the exact transition
   plan) in the same PR so no required check names a job that no longer
   exists after the install.
7. Update the CONSUMER-OWNED lint/test expectations that assert the shape of
   installer-overwritten workflows, so the sprint branch CI passes:
   `.just/lint-config.toml` (its `[version_sync.release_wiring]`
   `required_fragments` hardcode legacy `update-homebrew:` fragments of
   `release.yml`; `.just/check_version_sync.py` cannot pass otherwise) and
   the six failing assertions in `.just/tests/test_release_preflight.py` and
   `.just/tests/test_release_homebrew_workflow.py`. These are ATM-owned
   validation data (ADR-050), NOT kit files — updating them is permitted and
   required. Retention-vs-deletion decisions about those test files still
   belong to AT.3.

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

## Publish Surface Ground Truth

Deliverable 2's authored JSON must be diffed against this enumeration,
derived from the workspace via `cargo metadata --no-deps --format-version 1`
(package names, `publish` flags, and bin targets) plus the three
`pyproject.toml` files. This table is the authoring checklist; `cargo
metadata` at sprint time is authoritative if the workspace has changed.

| Surface | Members |
| --- | --- |
| Publishable crates (crates.io), dependency order | 12 crates, one valid topological order: `atm-error` → `atm-storage` → `agent-team-mail-core` → `atm-storage-rusqlite` → `atm-http-runtime` → `atm-runtime` → `atm-template-sc-compose` → `atm-daemon-bootstrap` → `atm-daemon-client` → `agent-team-mail` → `atm-daemon` → `atm-graft`. Any order satisfying the workspace dependency graph is acceptable. |
| Internal `publish = false` crates | 9 crates, never published: `atm-architecture`, `atm-storage-sqlserver-proof`, `atm-runtime-test-support`, `atm-peer-tls-interop`, `atm-graft-python`, `atm-query-python`, `sc-lint-attributes`, `sc-lint-directives`, `sc-lint-boundary`. |
| Python distributions | `atm-graft` (maturin, `crates/atm-graft-python`, version dynamic from Cargo.toml), `atm-query` (maturin, `crates/atm-query-python`), `hermes-atm` (setuptools, `crates/hermes-atm` — a pure-Python package, not a workspace crate). |
| Released binaries | `atm` (crate `agent-team-mail`) and `atm-daemon` (crate `atm-daemon`), both confirmed cargo bin targets. `atm_post_send_hook_fixture` and `atm-daemon-benchmark` are internal bins and are not released. |

Expected channel enablement (all six):

| Channel | Expected enablement |
| --- | --- |
| crates.io | Enabled — the 12 publishable crates above, in dependency order. |
| GitHub Releases | Enabled — both binaries across the declared release targets. |
| PyPI | Enabled — all three Python distributions; TestPyPI as the rehearsal target. |
| Homebrew | Enabled — tap `randlee/homebrew-tap` formulas. |
| Winget | Enabled — identifier `randlee.agent-team-mail`. |
| Scoop | Enabled — bucket `randlee/scoop-bucket`; kit-new channel with no legacy workflow. |

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

- `install.py --dry-run` (run from the pinned checkout) reports no drift
  after installation.
- Every copied shared file byte-compares equal to its package source; the only
  consumer-specific outputs are the installer-designated manifests.
- The complete JSON declares all required channels — crates.io, GitHub
  Releases, PyPI, Homebrew, Scoop, and winget, per the Ground Truth channel
  table — and preserves the legacy package names `agent-team-mail` and
  `agent-team-mail-core`; manifest rendering fails if any required channel or
  legacy name is missing (REQ-P-RELEASE-001/002/003/005).
- The complete JSON and parsed manifests agree on publish surface, explicit
  order, all Python distributions, and each declared build system.
- No branch-protection required check on `integrate/phase-at` or `develop`
  references a job name that no longer exists after the install
  (Deliverable 6).
- A fixture containing both the `maturin` and `setuptools` forms proves the
  shared workflow selects the declared backend and rejects an absent or
  unsupported build system before any upload.
- Preflight and release resolve the same package revision, manifest digest,
  and tool bootstrap; tests fail if either path selects a different source.
- Version-sync validation proves all released crates and Python distributions
  resolve to one workspace version.

## Required Validation

All kit commands run from the pinned `sc-publish` checkout, never the
vendored consumer-root `install.py` (sc-publish#46; see Deliverable 1). Each
command below must exit 0 on a clean install.

```bash
<venv>/bin/python <sc-publish-checkout>/plugins/sc-publish/install.py --dry-run --input release/sc-publish-consumer-input.json <worktree>
# Package-byte parity: byte-compare every copied kit file against its package
# source (the rehearsal's sweep: same file set and exclusion rules as
# install.py; expected result "byte-identical: 48, mismatched/missing: 0").
# A plain `diff -ru` is acceptable only if it excludes ALL consumer-owned
# files under release/ (publish-artifacts.toml, publish-channel-contracts.toml,
# sc-publish-consumer-input.json, and any other consumer-owned release/ file)
# so that it exits 0 on a clean install.
python3 .github/scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
python3 .just/check_version_sync.py
```

Run the package's own installer/unit-test suite (the mixed maturin/setuptools
fixture and the unsupported-build-system rejection tests live in that suite)
with the exact command proven by the rehearsal, from the `sc-publish`
checkout at the pinned revision — not from the consumer:

```bash
<venv>/bin/python -m pytest <sc-publish-checkout>/plugins/sc-publish/.github/scripts/tests/ -q
```

Its pass at the pin is recorded in the AT.1 receipt. ATM does not duplicate
that shared test suite.

## Required Document Updates

- Record the immutable `sc-publish` revision, package digest, complete
  consumer-input digest, and generated-manifest digests in the AT.1 receipt
  at `docs/plans/phase-at/receipts/AT.1-receipt.md` (shape per the phase
  README's Receipt Convention), and update the phase README pin.
- Record any shared-package defect as an upstream `sc-publish` issue/PR with
  the failing fixture and command; do not patch its copied ATM files
  (ADR-050).

## Rollback

If the installed kit path cannot pass Required Validation and the defect is
upstream (in the shared package, not the consumer input):

1. File the upstream `sc-publish` issue with the failing fixture and command.
2. Then either wait for the upstream fix and re-pin (restarting AT.1's
   proof), or `git revert` the install commits on the sprint branch —
   restoring the legacy workflows byte-for-byte. Never hand-edit a copied
   shared file to limp past the failure.
3. Do not merge the sprint PR in this state; escalate to team-lead.
4. If the sprint is abandoned, the sprint branch is retired through the
   standard worktree-abort flow (`sc-worktree-abort`). `develop` is never
   touched.

## Risks And Watchouts

- A package or installer defect is reported and fixed in `sc-publish`; AT.1
  must then repeat the install and proof against the new immutable revision.
- Upstream moves fast; adopting a newer revision restarts AT.1's proof and is
  never a reason to hand-merge changes.
- A credential result is not a package-install failure and is not grounds for
  inventing a local credential workaround.
- Classify every non-passing result against the phase README's Non-Blocking
  Outcomes table before reacting; only its GENUINE STOP row halts work.
