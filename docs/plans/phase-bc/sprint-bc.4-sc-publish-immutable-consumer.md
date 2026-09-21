# sprint bc.4 — consume a qualified immutable-release publish kit

Accountable owner: `cipher@atm-dev`.

## goal

Adopt the first qualified `sc-publish` revision that makes atm-core's release
pipeline compatible with GitHub immutable releases. Shared implementation is
fixed upstream under ADR-050, never locally patched here.

## entry gates

- The combined-stack `sc-publish` PR #106 has an exact-head acceptance verdict;
  PR #101 remains historical evidence only.
- `docs/plans/phase-bc/bc.4-upstream-owner-acceptance.md` records the accepted
  PR #106 scope and evidence handoff from the sc-publish maintainer.
- Credential appointment and repository-setting activation are not bc.4 entry
  gates; they are separately authorized in bc.5.
- Required follow-ups implement and test explicit draft-first stable and
  prerelease assembly, replacement refusal, exact tag/build binding, root
  per-tag concurrency, trusted production refs, and digest/checksum comparison.
  Post-publication receipt and attestation verification gates downstream
  promotion separately.
- A merged upstream commit has completed source, installed-consumer, hosted CI,
  and independent QA qualification.

## deliverables

1. Advance `release/sc-publish-pin.toml` to the exact qualified 40-character
   upstream commit.
2. Regenerate shared files only through the canonical pinned installer using
   `release/sc-publish-consumer-input.json`.
3. Prove package-byte parity, rendered manifest semantics, expected workflow
   inventory, and a clean second installer dry-run.
4. Update ATM-owned manifest/evidence/docs for the immutable-release contract.
5. Remove the functional `replace_release_assets` surface through the upstream
   generated result; retries become verify-only.
6. Add ATM-specific preflight checks for the exact tag/source/build/receipt
   without duplicating shared logic. Do not block or ask about tokens unless
   preflight or publish fails.
7. Record the exact merged revision, upstream QA, installed-consumer/package
   parity, canonical installer dry-run, and ATM validation in
   `docs/plans/phase-bc/bc.4-sc-publish-consumer-qualification.md`.

## acceptance

- No synced shared file has a consumer-only edit.
- Disabled, forbidden, timed-out, malformed, or indeterminate immutability API
  responses fail before any tag/registry/release/channel mutation.
- Stable and prerelease paths assemble drafts completely, verify assets and
  digests, then publish once.
- Exact tag, gated source, build checkout, and receipt SHAs agree under an
  approved migration that replaces ancestor-tag recovery without rewriting
  historical tags or releases.
- Same-tag retry verifies immutable state and cannot rebuild or replace bytes.
- Production post-release workflows reject untrusted dispatch refs.
- The bc.4 qualification artifact closes `u4` at the exact consumed revision.

## required validation

Run upstream qualification evidence checks, installer parity/dry-run checks,
ATM release-manifest and release-gate suites, mocked policy/permission/API
failures, duplicate dispatch, stale tag, partial draft, changed checksum, and
existing immutable release cases, followed by all phase gates.

## out of scope

Locally patching a synced shared file, enabling the repository setting,
publishing a release, provisioning credentials, mutating historical releases,
or promoting a registry/channel artifact.

Inherited or deferred FIX03–FIX10 work is not claimed by bc.4; its status is
recorded as upstream/deferred evidence in the qualification receipt.
