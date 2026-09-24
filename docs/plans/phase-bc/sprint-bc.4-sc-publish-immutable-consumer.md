# sprint bc.4 — consume a qualified immutable-release publish kit

Status: complete; u4 CLOSED at PR #1571 `b381e0981` (pin `f178b6919881c5a3d030d6343fcbb509f04806cc`).

Accountable owner: `cipher@atm-dev`.

## goal

Adopt the first qualified `sc-publish` revision that makes atm-core's release
pipeline compatible with GitHub immutable releases. Shared implementation is
fixed upstream under ADR-050, never locally patched here.

## entry gates

- The PR106-derived `sc-publish` head
  `22137c2da13bf4638b4267b69c6c2f021617da73` is frozen historical evidence;
  it is not a valid current u4 pin. sc-publish develop (`98a75ba3e`) and main
  were reconciled on 2026-09-23 UTC (sc-publish #98/#111 main->develop, #110
  develop->main); the qualified pin is the #110 merge commit on sc-publish
  main, `f178b6919881c5a3d030d6343fcbb509f04806cc`, adopted by PR #1571 (`b381e0981`).
- `docs/plans/phase-bc/bc.4-upstream-owner-acceptance.md` records the historical
  PR #106 scope and the superseding u4 boundary from the sc-publish maintainer.
- Credential appointment and repository-setting activation are not bc.4 entry
  gates; they are separately authorized in bc.5.
- FIX03–FIX10 are inherited or deferred upstream lifecycle items. They are
  recorded for boundary clarity and are not claimed as bc.4 work or entry
  gates. Post-publication receipt and attestation verification remain separate
  downstream work.
- The current source is sc-publish main `f178b6919881c5a3d030d6343fcbb509f04806cc`
  (develop content plus the prerelease reconciliation). PR #1571 edited the pin
  and performed the clean canonical install that closes u4; evidence is in
  `bc.4-sc-publish-consumer-qualification.md`.

## deliverables

1. `release/sc-publish-pin.toml` advanced to the exact qualified commit
   `f178b6919881c5a3d030d6343fcbb509f04806cc` (done, PR #1571).
2. Regenerate shared files only through the canonical pinned installer using
   `release/sc-publish-consumer-input.json`.
3. Prove package-byte parity, rendered manifest semantics, expected workflow
   inventory, and a clean second installer dry-run.
4. Update ATM-owned manifest/evidence/docs for the immutable-release contract.
5. Remove the functional `replace_release_assets` surface through the upstream
   generated result; retries become verify-only.
6. Add ATM-specific preflight checks for the exact tag/source/build/receipt
   without duplicating shared logic. Do not block or ask about tokens unless preflight or publish fails.
7. Record the exact qualified pinned revision, recorded upstream evidence,
   installed-consumer/package parity, canonical installer dry-run, and ATM
   validation in
   `docs/plans/phase-bc/bc.4-sc-publish-consumer-qualification.md`.

## acceptance

- No synced shared file has a consumer-only edit.
- The local negative suite verifies fail-closed handling for disabled,
  forbidden, timed-out, malformed, and indeterminate immutability/preflight
  results without performing a release or channel mutation.
- The consumer pin, generated output, package contract, and local installer
  qualification are recorded at sc-publish main `f178b6919881c5a3d030d6343fcbb509f04806cc`
  by PR #1571 (`b381e0981`); see the "current qualification" table in
  `bc.4-sc-publish-consumer-qualification.md`.
- BC4 does not assert implementation or live qualification of the inherited or
  deferred FIX03–FIX10 lifecycle items.
- The bc.4 qualification artifact records that clean pin/install evidence and
  closes u4; stack merge, hosted-QA, credentials, and live release/channel
  qualification remain outside this sprint.

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
