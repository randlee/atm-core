# AS.1 — Freeze the canonical publish-kit overlay

```yaml
plan_type: sprint_plan
phase: AS
sprint: AS.1
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pas-s1-overlay-contract
branch: feature/pas-s1-overlay-contract
status: in_progress
estimated_scope: documentation and upstream-contract closure
```

## Goal

Establish one reviewable, byte-exact adoption contract for the `sc-publish`
overlay before any ATM consumer activation.

## Scope Summary

The source SHA, owned paths, reasons for each path, and parity proof are the
deliverables. This sprint makes no release, workflow, action, helper, or
manifest behavior change in ATM.

## Governing Requirements

- `sc-publish/docs/publish-kit-requirements.md` is normative.
- The canonical `plugins/sc-publish/install.py` package installer is the only
  copier and parity checker. The prior standalone overlay script is retired.
- No copied path may be locally edited.

## Governing ADRs

- [ADR-050](../../adr/ADR-050-shared-publish-kit-ownership.md) establishes the
  shared implementation / consumer-data ownership boundary.

## Governing Boundaries

- Shared paths: upstream-owned and synchronized unchanged.
- ATM paths: manifest data and namespaced extension data only.

## Prerequisites

- `sc-publish` promotion baseline
  `240fd52` is reachable.
- The canonical package installer is available from that source.
- The current synchronization candidate is `sc-publish/develop@5e49b6ac` (PR
  #15). It is not accepted for activation until its generated artifact manifest
  passes the copied validator.

## Hard Dependencies

- `AS.2`: `must_follow`; it may only justify consumer data after AS.1 records
  the shared ownership contract.
- `AS.3`: `must_follow`; AS.3 may not be dispatched until every
  safety-load-bearing shared capability it relies on is implemented, accepted,
  and synchronized unchanged from `sc-publish`.

## Non-Goals

- Editing shared overlay files in ATM.
- Activating publication channels.
- Repairing upstream shared behavior locally.

## Sub-Tasks

1. Record the accepted source SHA and execute the canonical package installer
   twice: once to synchronize and once in `--dry-run` mode to prove no
   remaining byte difference.
2. Retain the 31-path audit in the supporting migration design as the
   complete decision record. The overlay is atomic: no individual path is
   selected, omitted, or patched locally.
3. File upstream requirements for every shared closure gap discovered by the
   audit: relative-dependency closure, installed-archive-member validation,
   resolved release-plan evidence, `extra_validations`, Cargo-derived dynamic
   PEP 621 versions, and an explicit installer-input contract.
4. Require an upstream closure test that fails whenever a copied shared file
   references an unvendored relative dependency not declared as a generic
   consumer interface.
5. Require the shared installer's sole consumer input to be complete declared
   data: source/version policy, artifacts, explicit crate dependency/publish
   order, wheels, binaries, channels, and channel settings. It must reject
   missing production input. Source discovery may produce an advisory example
   only; it must never infer a production publish surface, crate
   dependency/publish order, or enabled destination.
6. Require the shared bootstrap to provision the exact `sc-compose` CLI used
   by install/render and run the semantic installer integration test in CI
   through that bootstrap.
7. Require an installation proof that runs the installer with the ATM input,
   verifies every copied shared asset against the package source, parses the
   generated manifests, and proves their semantic values equal the supplied
   JSON. A second installer `--dry-run` must be clean.
8. Require upstream consumer CI coverage that fails on a byte difference in a
   synchronized file or any workflow-local tool installation that bypasses the
   common bootstrap. This is a shared-kit capability, not an ATM CI fork.
9. Classify each discovered gap as either advisory or safety-load-bearing. For
    a safety-load-bearing gap — including the resolved receipt fields
    `source_commit`, `manifest_sha256`, `toolchain_sha256`, and
    `validation_sha256` needed by AS.3/AS.4/AS.6 — require the capability to
    be implemented and accepted upstream, then synchronized unchanged, before
    dispatching the dependent sprint. An issue alone is not closure.

## Split Recommendation

Keep AS.1 isolated: it resolves ownership and evidence contracts only. Any
consumer manifest work belongs to AS.2.

## Acceptance Criteria

- The `240fd52` baseline and the accepted promotion SHA are recorded.
- Canonical installation followed by canonical `--dry-run` reports exact
  parity from one complete declared consumer input.
- The 31-path audit has a requirement and activation/proof reason for every
  path.
- Every advisory shared gap has an upstream issue or accepted disposition;
  every safety-load-bearing gap has an accepted upstream implementation and
  exact synchronized proof before its dependent sprint dispatches. There is no
  ATM workaround.
- AS.3 dispatch is blocked until the resolved receipt/digest mechanism exists
  upstream and its mismatch behavior blocks publication, records escalation,
  and requires a new matching preflight after correction.
- The upstream installer has no enabled-channel defaults and no production
  source-discovery fallback.
- One complete ATM JSON input builds every copied asset and both generated
  manifests to specification; generated manifest semantics equal that input.
- A second run in `--dry-run` mode is clean.
- The upstream semantic installation test passes in CI using the same pinned
  toolchain that consumers receive.

## Required Validation

```bash
PUBLISH_KIT_SOURCE=/Users/randlee/Documents/github/sc-publish
ATM_WORKTREE="$PWD"
git -C "$PUBLISH_KIT_SOURCE" rev-parse 240fd52
python3 "$PUBLISH_KIT_SOURCE/plugins/sc-publish/install.py" --help
```

## Required Document Updates

- Record the accepted source SHA and upstream issue/PR identifiers here.
- Update the 31-path supporting audit only when the upstream owned-path list
  changes.

## AS.1 Evidence And Upstream Closure Ledger

- Accepted source: `sc-publish/develop@240fd525784d967a993f04eab4ffcc0993933f7b`
  (the `240fd52` baseline); installer input/manifest proof:
  [`AS.1-consumer-input.json`](./evidence/AS.1-consumer-input.json).
- Accepted installer contract: `sc-publish` PR #7, merged at `240fd52`.
  It requires explicit `--input` for installation; `--example-json` can only
  create a reviewable draft and cannot install or infer a production surface.
- The 31-path atomic audit and legacy-value coverage remain in the supporting
  [migration design record](../publish-kit-migration/README.md). No individual
  shared path was selected, omitted, or edited locally.

| Upstream item | Classification | AS.1 disposition / dispatch effect |
| --- | --- | --- |
| `sc-publish` #6 — unified bootstrap and tool receipt | safety-load-bearing | Must be accepted, synchronized, and evidenced before AS.3. |
| `sc-publish` #9 — relative-dependency closure | safety-load-bearing | Must be accepted and synchronized before AS.3. |
| `sc-publish` #10 — installed archive-member validation | safety-load-bearing | Must be accepted and synchronized before AS.3. |
| `sc-publish` #11 — resolved fail-closed receipt | safety-load-bearing | Blocks AS.3 until implemented, accepted, synchronized, and proven. Required fields are `source_commit`, `manifest_sha256`, `toolchain_sha256`, and `validation_sha256`. |
| `sc-publish` #12 — `extra_validations` runner/evidence | safety-load-bearing | Must be accepted and synchronized before AS.3. |
| `sc-publish` #13 — Cargo-derived dynamic PEP 621 validation | safety-load-bearing | Must be accepted and synchronized before AS.3. |
| `sc-publish` #14 — generic release-version wiring | safety-load-bearing | Atomic sync revealed that ATM's legacy version check inspects an ATM-only workflow job. The generic manifest/workflow contract must replace that assertion before AS.3. |
| Explicit installer input / generated-manifest parity | blocked | PR #7 at `240fd52` supplies explicit input and PR #15 at `5e49b6ac` closes the source-layout defect. PR #16 fixes the `[[crates]]` entry shape. The remaining complete-schema gap — rendered `[project]`, channel, and Python-distribution fields required by the canonical validators — is tracked by [`sc-publish` #17](https://github.com/randlee/sc-publish/issues/17) and the consolidated report to its maintainer. AS.2 owns ATM's complete declared input; no local adapter is permitted. |

The safety items are intentionally **not** represented as closed by their
issue creation. AS.3 remains blocked until the stated upstream implementation
and exact consumer sync proof exist. For any receipt mismatch, publication is
blocked; record escalation upstream and obtain a new matching preflight after
the correction is accepted.

Historical AS.1 canonical-install result: the installer and clean second
`--dry-run` passed at `240fd52`. This was byte parity, not the required
execution-level validation.
AS.1 QA-1 corrected the ATM-owned version-sync assertion: it now requires the
canonical requested-version and lockstep checks rather than the retired
ATM-specific `update-homebrew` inline job. The assertion continues to check
release wiring; it is not disabled or weakened.

AS.1 QA-1 also identified a **critical upstream installer layout defect**:
the canonical source at `240fd52` copies its new helper implementations to
`.github/scripts/`, but the same canonical workflows invoke `scripts/`. The
consumer's existing `scripts/` helpers are legacy and lack the invoked
subcommands; its legacy `release_gate.sh` also has a three-argument interface
while canonical `release.yml` passes four arguments. This cannot be repaired
in ATM without locally editing a synchronized workflow or helper, which is
prohibited by AS.1. `sc-publish` PR #15 closed that source-layout defect at
`5e49b6ac`; ATM synchronized it unchanged and the second canonical `--dry-run`
reported `Publish-kit assets are in sync.`

The resulting execution proof exposed a second critical upstream contract
defect: the installer rendered `[[artifacts.crates]]` in
`release/publish-artifacts.toml`, but the copied
`.github/scripts/release_artifacts.py` requires top-level `[[crates]]`.
Consequently, both of the following canonical consumer commands fail with
`manifest must define [[crates]]`:

```bash
python3 .github/scripts/release_artifacts.py validate-manifest \
  --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
python3 .github/scripts/release_artifacts.py validate-publish-order \
  --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
```

The upstream installer suite passed while missing this cross-asset invariant.
AS.1 remains in progress until `sc-publish` fixes schema parity, adds a test
which renders a complete input and then runs both validators, and ATM performs
another unchanged canonical synchronization plus the path, argument, and
semantic proof.

## Risks And Watchouts

Do not represent a filed issue, current unit-test pass, or an in-sync file
diff as release readiness when a later sprint relies on an unimplemented
safety capability.
