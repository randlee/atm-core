# sc-publish pr #101 atm-dev acceptance

> **SUPERSEDED-BY-u4.** This is historical prerequisite-only evidence. The
> 2026-09-23 sequencing amendment names the reconciled sc-publish main pin
> `f178b6919` and PR #1571 as the current consumer gate; no u1/u2/u3 claim is
> reopened by this record.

## reviewed revision

- repository: `randlee/sc-publish`
- pull request: `#101`
- exact head: `34feb1a4e158a2b8eebbeed59791e39f610e6796`
- atm-dev reviewer: `arch-ctm@atm-dev`
- upstream cross-check: `aobs@sc-obs`
- review date: 2026-09-20

## verdict

`accept-as-prerequisite-only`

The exact head is a useful read-only, fail-closed prerequisite, but it is not a
qualified consumer pin and does not authorize immutable-release activation.
Hosted CI passed at the reviewed head. The upstream blocking review is recorded
at <https://github.com/randlee/sc-publish/pull/101#issuecomment-5751348377>.

## accepted coverage

- rejects release-asset replacement;
- reads the repository immutable-release setting and fails closed when the
  prerequisite cannot be established;
- propagates channel evidence; and
- records a finalized immutable-release receipt.

## blockers and uncovered contract

The stock `${{ github.token }}` cannot read the immutable-release setting
because the endpoint requires repository Administration(read). A separately
approved, provisioned, least-privilege runtime credential is therefore an
unresolved gate.

The reviewed head does not provide the full phase-bc publication contract:

- explicit draft-first stable and prerelease assembly;
- exact tag, gated-source, build-checkout, and receipt equality;
- root per-tag concurrency;
- comprehensive trusted production-dispatch ref enforcement;
- downloaded checksum and API asset-digest agreement; or
- post-publication attestation verification before downstream promotion.

The exact-source policy also replaces the existing ancestor-tag recovery
behavior. That migration requires an explicit upstream decision and tests; it
must not rewrite any historical release.

## consumer gate

The historical consumer gate is superseded by u4. atm-core may consume only
the exact u4 revision recorded in the phase-bc plan and current qualification
receipt; u4 is the sole sc-publish consumer gate. This record does not
authorize a setting write, credential change, tag, release, registry publish,
or channel mutation.
