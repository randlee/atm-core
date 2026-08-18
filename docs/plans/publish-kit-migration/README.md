# Shared Publish-Kit Migration Plan

**Branch:** `plan/sc-compose-publish-kit-migration`
**Status:** proposed — planning only

## Goal

Converge ATM on the unmodified shared `sc-compose` publishing kit and make a
single manifest-derived toolchain and validation plan govern both preflight and
publish. A release is not eligible until this plan's proof gates pass.

## Invariants

1. Shared-kit files are copied by one ATM sync script from an immutable,
   recorded `sc-compose` commit. `--check` compares bytes. No copied file is
   patched locally.
2. ATM-specific policy lives only in ATM manifest data or ATM-namespaced
   extension scripts; it never forks a shared workflow, action, or helper.
3. Preflight and publish use the same source commit, manifest digest,
   resolved-toolchain digest, and full validation list.
4. Production publish remains an explicit authorization after a real GitHub
   preflight and authorized TestPyPI proof.

## Shared-kit freeze rule

ATM must **not** modify shared `sc-compose` code, workflows, actions,
contracts, tests, or copied files. The only allowed ATM-side changes are:

- byte-for-byte synchronization from an accepted upstream source commit;
- ATM manifest data; and
- clearly namespaced ATM-only adapters outside the shared path list.

If a preflight or publishing defect appears to require a shared-file change,
the work stops. The defect and the required behavior are recorded in this plan,
then proposed and resolved upstream in `sc-compose`. ATM adopts the next
accepted source revision by sync; it does not make a local workaround.

The normative upstream source is `sc-compose/docs/publish-kit-requirements.md`.
Its channel contract is `release/publish-channel-contracts.toml`; credentials,
environments, public registry endpoints, and liveness semantics belong there,
not in ATM's artifact manifest or workflow literals.

## Unresolved preflight findings — no local workaround

| Finding | Observed evidence | Required resolution before any ATM change |
| --- | --- | --- |
| PF-1 environment-secret check | `GITHUB_TOKEN` receives HTTP 403 when listing protected environment-secret metadata. | Adopt the existing plan-owned GitHub variable/credential verification workflow, which verifies required values exist and are current without disclosing them. Do not replace it with environment-secret listing or a local redesign. |
| PF-2 crates.io state/liveness check | The raw `/api/v1/me` curl returns 403 while the same configured credential successfully published crates using Cargo. | The `crates-io` channel agent receives the manifest-derived crate list and emits a fenced JSON receipt for **every** crate: crate name, current published version(s), newest/current version, and last-publish timestamp. Preflight consumes that receipt for registry state instead of treating a raw `/api/v1/me` probe as equivalent to Cargo publish authentication. The receipt contains no credential value. Do not rotate or replace a working production token based on the raw curl. |
| PF-3 registry state | Candidate `1.4.3` is already published for all release crates. | Treat as the correct fail-closed result. Run preflight only with an unpublished candidate version; do not weaken the registry check. |

These findings are inputs to the upstream design review, not authorization for
ATM workflow edits.

## Ownership boundary

| Surface | Owner | Rule |
| --- | --- | --- |
| Generic workflows/actions, generic release-manifest parser, generic artifact helper, generic tests | `sc-compose` | Exact import only. Any required change is an upstream PR first. |
| ATM artifact/channel declarations and ATM validation commands | ATM | Data in `release/publish-artifacts.toml`; no workflow fork. |
| ATM installed-document helpers | ATM | Extract to `scripts/atm_release_artifacts.py`; ATM callers import that module. |
| Credential approval and execution | `publisher` / GitHub environment | Publisher supplies evidence; a person authorizes real publication. |

The frozen source revision is selected after its `sc-compose` PR is accepted.
ATM's sync metadata records that SHA and the complete owned-path list.

## Required shared-kit contract

### One resolved plan

The generic helper must resolve one machine-readable plan with the source
commit, requested version, enabled channels/artifacts in dependency order,
toolchain, and all validations. It emits the manifest and toolchain digests.
Release refuses to publish unless they equal the successful preflight receipt.

### One tool bootstrap

The shared kit must own one reusable bootstrap action/resolver which installs
every declared tool at its exact version (or immutable action revision).
Preflight and all release jobs call it. No workflow-local `pip install`,
`cargo install`, `cargo binstall`, or differing tool pin remains outside that
bootstrap. It emits a receipt of executable paths and resolved versions.

This specifically prevents the earlier divergence between preflight and
publish tool versions (`sc-compose`, `codespell`, Python, Rust, and cargo
tools).

### Manifest-defined extra validations

`sc-compose` must support generic `[[extra_validations]]` entries:

- `id`, unique and stable;
- argv `command`, never prompt prose;
- `phases` (`preflight`, `release`, or both);
- `required`, `timeout_seconds`, and `evidence_path`;
- `description`.

The generic runner executes every declared validation, writes structured
results for all of them, then returns a fail-closed verdict. It must not infer
synthetic task inputs, recipients, credentials, or repository names.

### Channels are manifest data

Crates.io, PyPI/TestPyPI, Homebrew, Scoop, and future channels are declared in
the manifest. Their shared workflow phases stay generic. GitHub environment
configuration names secrets; no personal identifiers, tokens, or chat IDs
enter shared files.

## Migration work

### M1 — Freeze and prove the source boundary

1. Add `scripts/sync_sc_compose_publish_kit.py` with a reviewed hardcoded list
   of shared paths, recorded source SHA, copy mode, and byte-for-byte `--check`.
2. Test copy-then-check and a one-byte negative mutation.
3. CI runs `--check` before release validation.
4. Any required shared-file change goes to `sc-compose`, then updates the
   recorded SHA. No ATM edit to a copied path is allowed.

### M2 — Remove known consumer collisions

1. Move ATM installed-document functions out of shared
   `scripts/release_artifacts.py` into `scripts/atm_release_artifacts.py`.
2. Update `scripts/validate_release.py` and
   `scripts/verify_release_archive.py` to import the ATM module.
3. Add focused equivalence tests and run current release-validator tests before
   and after extraction.

This is a consumer-boundary adapter, not a shared-kit modification.

### M3 — Exact adoption and obsolete-code removal

1. Sync from the frozen source revision.
2. Redirect every caller to the copied kit.
3. Delete superseded ATM workflow/action/helper copies only after callers and
   parity tests pass.
4. Add a digest test for every shared path listed in sync metadata.

### M4 — Make the manifest the sole policy input

1. Land `toolchain` and `extra_validations` schema/parser/runner/tests in
   `sc-compose` first.
2. Declare every ATM tool, channel, and ATM-only validation in
   `release/publish-artifacts.toml`.
3. Have both preflight and release consume the same resolved plan and upload
   their receipts.
4. Add static tests rejecting workflow-local tool installation and requiring
   the single bootstrap in every preflight/release job.

### M5 — Prove equivalence before production

1. Run shared-kit tests inside ATM after exact copy.
2. Run `just lint` and `just test`.
3. Dispatch the real `release-preflight.yml` on the migration branch; retain
   plan, tool-receipt, and validation-result artifacts.
4. Verify every receipt has identical source, manifest, and toolchain digests
   and that every required validation ran.
5. Run an explicitly authorized TestPyPI/dry-run release. Release must reject
   mismatched preflight evidence.
6. Install/smoke-test generated `hermes-atm` and `atm-graft` artifacts on
   Python 3.11, 3.12, 3.13, and 3.14.
7. Run each enabled channel verification from manifest data; disabled channels
   require an explicit recorded waiver, never silent omission.

## Completion gate

- [ ] Accepted `sc-compose` SHA is recorded and reachable.
- [ ] Exact sync and `--check` pass; shared-path digests all match.
- [ ] ATM extensions are outside the copied path set and independently tested.
- [ ] Shared `toolchain` and `extra_validations` contracts are accepted upstream.
- [ ] Every preflight/release job calls one bootstrap; no local tool install.
- [ ] Preflight/release receipts verify the same plan and toolchain digests.
- [ ] Shared-kit tests, `just lint`, `just test`, and a real preflight pass.
- [ ] An authorized TestPyPI end-to-end run succeeds under that same plan.
- [ ] Independent QA confirms source parity and no release-design regression.

## Non-goals and rollback

This plan does not authorize editing copied shared files, production publishing
without evidence, or embedding repository/personal data in the shared kit.
Before production, reverting the adoption commit restores the current release
path. Afterwards, correct a regression by reverting the recorded source SHA or
adopting a later accepted source SHA through the sync script—never by patching
a copied file in place.
