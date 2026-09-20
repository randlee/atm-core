# sprint bc.5 — enable immutable releases and retain proof

Accountable owner: `solar@atm-dev`. The repository administrator who performs
the setting write must be explicitly appointed before this sprint starts.

## goal

Enable GitHub immutable releases for `randlee/atm-core` only after compatible
release code and credentials are deployed, then prove the first future release
meets the complete invariant.

## preconditions

1. bc.4-compatible release code is present on every reachable release writer,
   including default, manual, prerelease, and channel paths, not only on a
   feature or nominal release branch.
2. Required repository credential/environment configuration is installed and
   preflighted without exposing secrets.
3. A rollback/repair plan exists for failed drafts. Published immutable
   releases are never repaired by mutation.
4. Rand explicitly authorizes the external repository-setting write and any
   release publication. This plan is not that publication authorization.

## deliverables

1. Record the pre-change API response.
2. Enable the repository immutable-release setting through the reviewed admin
   path and record `enabled=true` plus `enforced_by_owner`.
3. Run non-mutating readiness preflight with the production credential path.
   Store sanitized credential-path evidence in
   `docs/plans/phase-bc/bc.5-credential-preflight.md`.
4. Before the next separately authorized release, gate on setting, trusted
   workflow, provenance, credential, exact source, and a complete draft. After
   publication, retain tag/source/build/receipt, asset list, API digests,
   `checksums.txt`, immutable status, and `gh release verify` evidence before
   downstream registry or channel promotion.
5. Demonstrate through safe API/refusal probes that published asset replacement
   and tag movement are denied; do not delete real assets or tags for testing.
6. Document that historical releases remain mutable and unsupported for repair.
   Store the sanitized setting and first-release evidence in
   `docs/plans/phase-bc/bc.5-immutable-release-evidence.md`.

## acceptance

- The setting is enabled and the release workflow can read it at runtime.
- The first future release reports immutable and has a valid release
  attestation.
- Receipt and attestation verification gate downstream promotion and do not
  circularly gate the first publication.
- Every manifest asset digest matches the checksum map.
- Same-tag rerun is verify-only.
- No tag, release, registry, or channel mutation was performed without explicit
  operator authorization.

## required validation

Run repository-setting GET verification, release preflight, exact receipt and
digest checks, `gh release verify`, and the safe negative/refusal matrix. Store
sanitized evidence in the sprint artifact named by the implementation plan.

## out of scope

Publication without separate contemporaneous authorization, mutation or
repair of a published release, destructive denial testing against a real tag
or asset, credential disclosure, or organization-wide policy changes.
