# sprint bc.4 — consume a qualified immutable-release publish kit

Status: implementation present; QA-1 remediation active; u4 open.

Accountable owner: `cipher@atm-dev`.

## goal

Adopt the first qualified `sc-publish` revision that makes atm-core's release
pipeline compatible with GitHub immutable releases. Shared implementation is
fixed upstream under ADR-050, never locally patched here.

## entry gates

- The PR106-derived `sc-publish` head
  `22137c2da13bf4638b4267b69c6c2f021617da73` is frozen historical evidence;
  it is not a valid current u4 pin. The active merged PR99/100/101/108 stack
  reports accepted develop head `98a75ba3e`; a clean consumer layer must pin
  and reinstall it before u4 closes.
- `docs/plans/phase-bc/bc.4-upstream-owner-acceptance.md` records the historical
  PR #106 scope and the superseding u4 boundary from the sc-publish maintainer.
- Credential appointment and repository-setting activation are not bc.4 entry
  gates; they are separately authorized in bc.5.
- FIX03–FIX10 are inherited or deferred upstream lifecycle items. They are
  recorded for boundary clarity and are not claimed as bc.4 work or entry
  gates. Post-publication receipt and attestation verification remain separate
  downstream work.
- The current source is the merged sc-publish develop result at
  `98a75ba3e`; this layer does not edit the pin or claim the clean consumer
  install required to close u4.

## deliverables

1. A later clean consumer layer must advance `release/sc-publish-pin.toml` to
   the exact merged develop commit; this QA remediation does not edit the pin.
2. Regenerate shared files only through the canonical pinned installer using
   `release/sc-publish-consumer-input.json`.
3. Prove package-byte parity, rendered manifest semantics, expected workflow
   inventory, and a clean second installer dry-run.
4. Update ATM-owned manifest/evidence/docs for the immutable-release contract.
5. Remove the functional `replace_release_assets` surface through the upstream
   generated result; retries become verify-only.
6. Add ATM-specific preflight checks for the exact tag/source/build/receipt
   without duplicating shared logic. Do not block or ask about tokens unless preflight or publish fails.
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
  qualification remain pending at the merged develop source head.
- BC4 does not assert implementation or live qualification of the inherited or
  deferred FIX03–FIX10 lifecycle items.
- The bc.4 qualification artifact must remain open until the clean merged-
  develop pin/install evidence exists; merge, hosted-QA, credentials, and live
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
