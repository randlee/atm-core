# Shared Publish-Kit Migration Plan

**Branch:** `plan/sc-compose-publish-kit-migration`
**Status:** proposed — planning only

## Goal

Converge ATM on the unmodified shared `sc-publish` publishing kit and make a
single manifest-derived toolchain and validation plan govern both preflight and
publish. A release is not eligible until this plan's proof gates pass.

## Invariants

1. Shared-kit files are copied only by the canonical `sc-publish` sync tool
   from an immutable, recorded source commit. Its `--dry-run` diff and
   verification mode establish exact parity. No copied file is patched locally.
2. ATM-specific policy lives only in ATM manifest data or ATM-namespaced
   extension scripts; it never forks a shared workflow, action, or helper.
3. Preflight and publish use the same source commit, manifest digest,
   resolved-toolchain digest, and full validation list.
4. Production publish remains an explicit authorization after a real GitHub
   preflight and authorized TestPyPI proof.

## Shared-kit freeze rule

ATM must **not** modify shared `sc-publish` code, workflows, actions,
contracts, tests, or copied files. The only allowed ATM-side changes are:

- byte-for-byte synchronization from an accepted upstream source commit;
- ATM manifest data; and
- clearly namespaced ATM-only adapters outside the shared path list.

If a preflight or publishing defect appears to require a shared-file change,
the work stops. The defect and the required behavior are recorded in this plan,
then proposed and resolved upstream in `sc-publish`. ATM adopts the next
accepted source revision by sync; it does not make a local workaround.

The normative upstream source is `sc-publish/docs/publish-kit-requirements.md`.
Its channel contract is `release/publish-channel-contracts.toml`; credentials,
environments, public registry endpoints, and liveness semantics belong there,
not in ATM's artifact manifest or workflow literals.

## Unresolved preflight findings — no local workaround

| Finding | Observed evidence | Required resolution before any ATM change |
| --- | --- | --- |
| PF-1 environment-secret check | `GITHUB_TOKEN` receives HTTP 403 when listing protected environment-secret metadata. | Adopt the existing plan-owned GitHub variable/credential verification workflow, which verifies required values exist and are current without disclosing them. Do not replace it with environment-secret listing or a local redesign. |
| PF-2 crates.io state/liveness check | The raw `/api/v1/me` curl returns 403 while the same configured credential successfully published crates using Cargo. | Adopt the already implemented and tested shared `crates-io` agent receipt. For every manifest crate it returns fenced JSON with crate name, current published version(s), newest/current version, last-publish timestamp, classification (`new`, `existing`, `version_conflict`), and manifest-derived planned action (`publish`, `skip_already_at_target`, `block_conflict`). ATM's only work is exact sync plus a consumer proof that preflight consumes this receipt. Do not treat raw `/api/v1/me` as Cargo-publish authentication or rotate a working token because it fails. |
| PF-3 registry state | Candidate `1.4.3` is already published for all release crates. | Treat as the correct fail-closed result. Use the per-artifact fenced receipt and manifest action to publish only artifacts missing at the requested version, skip those already at target, and block true version conflicts. A synchronized version bump is one explicit release choice, not an automatic workflow response; do not weaken the registry check. |

These findings are inputs to the upstream design review, not authorization for
ATM workflow edits.

## Ownership boundary

| Surface | Owner | Rule |
| --- | --- | --- |
| Generic workflows/actions, generic release-manifest parser, generic artifact helper, generic tests | `sc-compose` | Exact import only. Any required change is an upstream PR first. |
| ATM artifact/channel declarations and ATM validation commands | ATM | Data in `release/publish-artifacts.toml`; no workflow fork. |
| ATM installed-document helpers | ATM | Extract to `scripts/atm_release_artifacts.py`; ATM callers import that module. |
| Credential approval and execution | `publisher` / GitHub environment | Publisher supplies evidence; a person authorizes real publication. |

The frozen source revision is selected after its `sc-publish` PR is accepted.
The canonical sync tool's manifest records that SHA and the complete owned-path
list; ATM does not maintain a second sync implementation or metadata format.

## Canonical overlay audit

The canonical `sc-publish` `sync-overlay.sh --dry-run` initially found 31 ATM
differences. Each has been reviewed below. They are one atomic, byte-for-byte
overlay: ATM must not select or edit individual files. A channel remains dormant
until ATM declares it in its repository-specific artifact manifest.

| Canonical path | Why it is required | Activation / proof |
| --- | --- | --- |
| `.claude/agents/publisher.md` | Replaces ATM-specific release prose with the manifest-driven coordinator contract. | Publisher evals and structured receipt. |
| `.claude/agents/publisher-channel-protocol.md` | Defines evidence-gated worker behavior and isolated retry. | Channel-worker eval. |
| `.claude/agents/crates-io-publisher.md` | Owns crates inquiry, publication, and partial retry. | Existing shared crates receipt. |
| `.claude/agents/github-release-publisher.md` | Owns immutable GitHub Release creation only. | Root-release preflight. |
| `.claude/agents/homebrew-publisher.md` | Owns manifest-declared Homebrew work. | Dormant unless Homebrew is declared. |
| `.claude/agents/pypi-publisher.md` | Owns normalized PyPI/TestPyPI inquiry and publication. | Authorized TestPyPI rehearsal. |
| `.claude/agents/scoop-publisher.md` | Owns manifest-declared Scoop work. | Dormant unless Scoop is declared. |
| `.claude/agents/winget-publisher.md` | Owns manifest-declared winget work. | Dormant unless winget is declared. |
| `.claude/skills/publishing/SKILL.md` | Provides the shared launch and release discipline. | Publishing skill evaluation. |
| `.claude/skills/publishing/agents/openai.yaml` | Registers the shared skill for discovery. | Skill discovery check. |
| `.claude/skills/publishing/evals/channel-name-inquiry.md` | Tests read-only registry inquiry delegation. | Shared evaluation. |
| `.claude/skills/publishing/evals/publisher-preflight.md` | Tests non-disclosing complete preflight behavior. | Shared evaluation. |
| `.claude/skills/publishing/evals/publisher-recovery.md` | Tests retry of only failed channels. | Shared evaluation. |
| `.claude/skills/publishing/preflight.xml.j2` | Supplies the canonical preflight task envelope. | Rendered-task test. |
| `.claude/skills/publishing/publish.xml.j2` | Supplies the canonical publish/retry task envelope. | Rendered-task test. |
| `.claude/skills/publishing/ref/channel-contracts.md` | Documents operation of the machine-readable channel contract. | Source-parity check. |
| `.claude/skills/publishing/ref/release-state-strategy.md` | Makes readiness and final-main preflights distinct. | Publisher preflight eval. |
| `.github/actions/extract-published-renderer/action.yml` | Safely extracts a verified release renderer for manifest-driven channels. | Channel workflow test. |
| `.github/actions/verify-published-release/action.yml` | Requires an immutable, non-draft release and declared assets. | Channel workflow test. |
| `.github/workflows/release-preflight.yml` | Runs shared complete, non-disclosing preflight and emits receipts. | Real preflight dispatch. |
| `.github/workflows/release.yml` | Uses the same resolved plan/tooling to build, publish, and release. | Preflight/release digest comparison. |
| `.github/workflows/pypi-publish.yml` | Allows PyPI/TestPyPI retry from immutable release assets. | Authorized TestPyPI rehearsal. |
| `.github/workflows/homebrew-publish.yml` | Allows independent manifest-driven Homebrew retry. | Dormant unless Homebrew is declared. |
| `.github/workflows/scoop-publish.yml` | Allows independent manifest-driven Scoop retry. | Dormant unless Scoop is declared. |
| `.github/workflows/winget-publish.yml` | Allows independent manifest-driven winget retry. | Dormant unless winget is declared. |
| `docs/publish-kit-requirements.md` | Carries the normative shared requirements in the consumer. | Review against upstream SHA. |
| `release/publish-channel-contracts.toml` | Centralizes channel names, secret names, environments, endpoints, and liveness rules. | Contract/parser tests. |
| `scripts/release_manifest.py` | Parses manifest plus channel contract without workflow literals. | Manifest parser tests. |
| `scripts/release_artifacts.py` | Resolves plans, receipts, registry checks, version checks, and release assets. | Shared helper test suite. |
| `scripts/release_gate.sh` | Enforces main/develop convergence and manifest version checks. | Gate test on an eligible candidate. |
| `scripts/tests/test_release_artifacts.py` | Regression coverage for the shared helper and manifest contract. | Run unchanged after sync. |

The source at `sc-publish` `develop` (`579b477d555b40754cba8243c7e72848e3590bca`)
was synchronized into the ATM staging worktree using only the canonical script;
a second `--dry-run` reported **in sync**. That proves source parity, not
runtime readiness.

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

### S1 — Justify and freeze the exact overlay

1. Review each canonical overlay path against the shared requirement and its
   QA/proof obligation; retain the 31-row audit above as the decision record.
2. Pin an accepted `sc-publish` source SHA and run its `--dry-run` before and
   after synchronization. A one-byte mutation must be detected by that same
   canonical tool.
3. Record that all copied paths are byte-identical; any needed shared change
   is an upstream `sc-publish` PR, never an ATM patch.

### S2 — Justify ATM manifest and activation changes

1. Compare ATM's existing artifact manifest to the canonical parser schema.
2. For every required ATM manifest addition, record the corresponding
   canonical workflow/helper that consumes it and the required proof.
3. Declare only ATM artifacts, destinations, and enabled channels; do not put
   credentials, tool versions, or workflow behavior in the manifest.
4. Keep every undeclared channel dormant and record why it is not enabled.

This sprint changes only ATM data after the justification record is accepted;
it does not modify any copied shared path.

### S3 — Exact adoption and obsolete-code removal

1. Sync from the frozen source revision.
2. Redirect every caller to the copied kit.
3. Delete superseded ATM workflow/action/helper copies only after callers and
   parity tests pass.
4. Use the canonical tool's source-revision and digest verification for every
   shared path; do not add a competing ATM parity implementation.

### S4 — Make the manifest the sole policy input

1. Land `toolchain` and `extra_validations` schema/parser/runner/tests in
   `sc-compose` first.
2. Declare every ATM tool, channel, and ATM-only validation in
   `release/publish-artifacts.toml`.
3. Have both preflight and release consume the same resolved plan and upload
   their receipts.
4. Add static tests rejecting workflow-local tool installation and requiring
   the single bootstrap in every preflight/release job.

### S5 — Prove equivalence before production

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
- [ ] The canonical sync tool's dry-run and parity verification pass; all
      shared-path digests match.
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
adopting a later accepted source SHA through the canonical sync tool—never by patching
a copied file in place.
