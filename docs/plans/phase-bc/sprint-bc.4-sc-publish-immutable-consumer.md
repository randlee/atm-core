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
- FIX03–FIX10 are inherited or deferred upstream lifecycle items. They are
  recorded for boundary clarity and are not claimed as bc.4 work or entry
  gates. Post-publication receipt and attestation verification remain separate
  downstream work.
- The accepted source is the open sc-publish head
  `22137c2da13bf4638b4267b69c6c2f021617da73`; bc.4 qualifies the exact pin,
  canonical installer/generated parity, and local consumer behavior. It does
  not claim that the source is merged or live-release qualified.

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
7. Record the exact accepted open revision, recorded upstream evidence,
   installed-consumer/package parity, canonical installer dry-run, and ATM
   validation in
   `docs/plans/phase-bc/bc.4-sc-publish-consumer-qualification.md`.

## acceptance

- No synced shared file has a consumer-only edit.
- The local negative suite verifies fail-closed handling for disabled,
  forbidden, timed-out, malformed, and indeterminate immutability/preflight
  results without performing a release or channel mutation.
- The consumer pin, generated output, package contract, and local installer
  qualification agree at the accepted open source head.
- BC4 does not assert implementation or live qualification of the inherited or
  deferred FIX03–FIX10 lifecycle items.
- The bc.4 qualification artifact closes the local-consumer portion of `u4` at
  the exact consumed revision; merge, hosted-QA, credentials, and live
  release/channel qualification remain outside this correction.

## required validation

Run the recorded upstream evidence check, the canonical installer parity and
repeat dry-run, the ATM release-manifest/release-gate suites, and the focused
mocked immutable/preflight negative suite. These are local evidence checks;
they do not perform live publication, channel promotion, or repository-setting
mutation.

## out of scope

Locally patching a synced shared file, enabling the repository setting,
publishing a release, provisioning credentials, mutating historical releases,
or promoting a registry/channel artifact.

Inherited or deferred FIX03–FIX10 work is not claimed by bc.4; its status is
recorded as upstream/deferred evidence in the qualification receipt.
