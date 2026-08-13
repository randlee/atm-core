---
title: AN.14 sc-compose 1.4.1 Checked-Emission Runtime Upgrade
status: blocked
branch: feature/an14-sc-compose-141-checked-emission
target: integrate/phase-an
external_blockers:
  - crates.io publishes sc-sha 1.4.1, sc-composer 1.4.1, and sc-compose 1.4.1; published sc-composer exports check_rendered_output, CheckedOutput, and OutputFormat
  - https://github.com/randlee/sc-compose/issues/448 closed
---

# AN.14 — sc-compose 1.4.1 Checked-Emission Runtime Upgrade

**recommended_agent:** arch-ctm/deep-reasoning (fail-closed rendering and
cross-route error behavior).
**must_follow:** AN.13 merged. Merge AN.13's pushed integration tip before
every dev/fix round because it owns the persisted `TemplateOutputFormat` that
prevents render-on-read from guessing a format. This sprint is also **blocked**
until crates.io publishes `sc-sha` **1.4.1**, `sc-composer` **1.4.1**, and
`sc-compose` **1.4.1**; the published `sc-composer` release exports
`check_rendered_output`, `CheckedOutput`, and `OutputFormat`; and
[sc-compose #448](https://github.com/randlee/sc-compose/issues/448) is closed
with the direct-library checked-emission regression test. Never bypass either
gate with a git revision, path dependency, prerelease, or version range.

**unblocks:** the checked-render portion of Phase AN close-out.
**parallel_safe:** none. AN.14 consumes AN.13's catalog contract and changes
the three production rendering routes as one fail-closed behavior change.

**traceability:** AN.13; Phase AN Decisions 2, 5, and 8; ADR-036; sc-compose
`docs/atm-adapter-notes.md`; and sc-compose #448.

## Deliverables

1. Replace the exact `=1.4.0` `sc-sha` and `sc-composer` pins owned only by
   `atm-template-sc-compose` with exact published `=1.4.1` pins and update
   `Cargo.lock`. Preserve the existing dependency prohibition: no other ATM
   crate directly depends on either upstream crate; no shell-out, local
   renderer/parser/hash, or local extension classifier is introduced.

2. In the sealed adapter only, call the released
   `sc_composer::check_rendered_output` after complete final-body assembly and
   before returning a rendered body. Feed it the AN.13 persisted format for
   stored/decomposed templates and the AN.13 adapter-derived format for
   file-backed templates. On success, return only the checked body; on failure,
   translate the upstream diagnostic once to the existing ATM error boundary,
   retain the cause, and identify the template SHA when available.

3. Apply Deliverable 2 to every production emission route: file-backed
   compose/send (including root-confined expansion), same-host decomposed
   render-on-read, and verified rendered fallback before it can be sent or
   persisted as plain text. A rejection performs no send, catalog/message
   mutation, cache/export write, or partial body emission.

4. Update the exact-pin manifest, boundary enforcement, error-code inventory,
   and adapter documentation to name the 1.4.1 checked-emission ownership and
   the legacy/unverified behavior established by AN.13.

## Acceptance criteria

- `Cargo.lock` contains the actual crates.io 1.4.1 sources/checksums; no
  git/path override, prerelease, or version range satisfies the criterion.
- A malformed JSON result is rejected for file-backed send, verified fallback,
  and stored/decomposed render-on-read. Each route proves no mutation,
  output/cache/export body, or rendered-variable leak.
- Valid JSON succeeds on every applicable route; text retains byte-for-byte
  behavior through the same checker. The vectors cover auto and legacy JSON
  escape modes, complete final-body assembly (guidance/prompt when applicable),
  and a multi-pass final-output failure with its failing pass reported.
- Existing legacy/unverified rows retain AN.13's documented compatibility
  behavior and never become evidence of 1.4.1 checked rendering; a newly
  re-registered classified row is checked after restart.
- Only `atm-template-sc-compose` imports upstream checked-render types and
  functions. Boundary lint, manifests, error docs, and all test doubles prove
  the adapter remains the single translation/ownership seam.

## Required validation

- `cargo test -p atm-template-sc-compose -p atm-core -p atm-storage-rusqlite`
  with positive and rejection vectors for all three production routes.
- `cargo test -p atm-architecture --test boundary_enforcement`, the exact-pin
  check, `just lint`, and `just test` on Linux, macOS, and Windows CI.
- Retained evidence: crates.io version/checksum lookup, closed sc-compose #448
  URL, upstream 1.4.1 API call sites, no-leak test output, and final CI commit.

## Paths to delete

None. Do not delete AN.1 oracle evidence or AN.13 legacy catalog rows.

## Non-closure

AN.14 does not change sc-compose's public API, make `compose()` return a
checked type, add non-JSON semantic validation, infer a format for legacy rows,
synchronize templates across hosts, or add workflow-specific behavior. Those
remain either upstream sc-compose work or explicit Phase AN non-goals.
