---
title: AN.13 sc-compose 1.4.1 Checked-Render Upgrade
status: blocked
branch: feature/an13-sc-compose-141-checked-render
target: integrate/phase-an
external_blockers:
  - sc-sha, sc-composer, and sc-compose 1.4.1 published on crates.io
  - https://github.com/randlee/sc-compose/issues/448 closed
---

# AN.13 — sc-compose 1.4.1 Checked-Render Upgrade

**recommended_agent:** arch-ctm/deep-reasoning (sealed renderer-port and
durable output-format contract).
**must_follow:** AN.10 merged, because AN.10 owns the atomic decomposed
admission path and `TemplateCatalogStore` contract this sprint extends. Before
every dev/fix round, merge the pushed AN.10 integration tip. In addition, this
sprint is **blocked** until all three external conditions hold:

1. `sc-sha` **1.4.1** and `sc-composer` **1.4.1** are published on crates.io;
2. the matching sc-compose **1.4.1** release is published and its public
   `check_rendered_output`, `CheckedOutput`, and `OutputFormat` contract is
   available from that release; and
3. [sc-compose #448](https://github.com/randlee/sc-compose/issues/448) is
   closed with direct-library regression coverage for the documented
   `compose` → `check_rendered_output` checked-emission sequence.

Do not use an unpublished git revision, a local path override, or a version
range to bypass these gates. Until they hold, AN.13 is planned-but-blocked and
no implementation PR may claim its acceptance criteria.

**unblocks:** the Phase AN renderer-upgrade close-out; it does not unblock the
workflow-metadata extension.
**parallel_safe:** none. AN.13 changes the same catalog/admission contract
AN.10 makes atomic and must be reviewed as one checked-emission boundary.

**traceability:** Phase AN Decisions 2, 3, 5, and 8; ADR-036; the existing
exact-pin policy in AN.1; `docs/atm-adapter-notes.md` in sc-compose; and
sc-compose #448.

## Deliverables

1. Replace the exact `=1.4.0` `sc-sha` and `sc-composer` pins owned only by
   `atm-template-sc-compose` with exact published `=1.4.1` pins, update
   `Cargo.lock`, and retain the architectural rule that no other ATM crate
   directly depends on either upstream crate. Do not add a shell-out to the
   `sc-compose` executable or duplicate a renderer, JSON parser, hash, or
   extension classifier in ATM.

2. Extend the core-owned `TemplateComposer` port, its sealed production
   adapter, the leaf template catalog DTOs, and the catalog schema so the
   adapter-derived output format is durable at template admission and available
   for every later render. The ATM type is deliberately small and does not
   expose upstream types outside the adapter boundary:

   ```rust
   /// Persisted identity of the output contract selected by the pinned adapter.
   pub enum TemplateOutputFormat {
       Text,
       Json,
   }

   pub struct TemplateInspection {
       pub sha: TemplateSha,
       pub frontmatter: TemplateFrontmatter,
       pub include_references: Vec<TemplateReference>,
       pub output_format: TemplateOutputFormat,
   }
   ```

   For a file-backed send, only `atm-template-sc-compose` calls the published
   `sc_composer::OutputFormat::from_template_path` API and translates its
   result once. The derived value is persisted with the immutable template
   record in the same catalog/admission transaction. Render-on-read constructs
   its source from that stored value. Neither core nor storage infers JSON from
   raw bytes, an arbitrary `metadata.name`, or a local filename heuristic.

3. Route all three production rendering seams through the released checked
   contract after final body assembly and before output is returned:

   - file-backed compose/send, including root-confined include expansion;
   - same-host stored/decomposed render-on-read; and
   - any verified rendered fallback that may be sent or persisted as plain
     text.

   `Json` calls `sc_composer::check_rendered_output` on the complete final
   body and may return only the successful checked body. `Text` preserves the
   existing rendering behavior through the same upstream checker. A rejected
   render is typed once at the adapter/core error boundary, preserves the
   upstream diagnostic as a cause, identifies the template SHA when available,
   and is neither sent, stored, cached, exported, nor partially emitted.

4. Add the additive catalog migration and DTO/trait/boundary records needed by
   Deliverable 2. New templates always persist `Text` or `Json`. Pre-AN.13
   catalog rows have no trustworthy filename/format identity, so their
   migration state is an explicit legacy/unverified state: they remain
   readable under the pre-AN.13 compatibility behavior but cannot be claimed
   as checked-render proof. The migration must not guess from template body or
   metadata. Document the operator migration/re-registration path that turns a
   legacy row into a newly admitted, format-classified row.

5. Update the exact-pin manifest, boundary-enforcement expectations, and
   error-code documentation together. `atm-template-sc-compose` remains the
   sole authorized implementation and upstream dependency owner; `atm-core`,
   `atm-storage`, `atm-storage-rusqlite`, CLI, daemon, runtime, and HTTP crates
   remain forbidden from importing `sc_composer` or `sc_sha`.

## Acceptance criteria

- The actual crates.io 1.4.1 source and checksums are locked in `Cargo.lock`;
  no git/path override or prerelease satisfies this criterion.
- sc-compose #448 is closed, and its merged upstream test demonstrates the
  direct-library malformed-JSON rejection path used by adapters. AN.13 records
  the closed issue URL and upstream release version in retained evidence.
- A JSON template that renders to invalid JSON is rejected for file-backed
  send, verified fallback, and stored/decomposed render-on-read. Each test
  proves no message/catalog mutation, output/cache/export body, or rendered
  variable value leaks after rejection.
- A valid JSON template succeeds on each applicable route; plain-text templates
  retain byte-for-byte behavior. Tests cover auto and legacy JSON escape mode,
  the fully assembled final body (including guidance/prompt where applicable),
  and a multi-pass final-output failure with the reported failing pass.
- File extension classification occurs solely through the released upstream API
  at the adapter boundary. Stored render-on-read uses the persisted format; no
  path/body/metadata inference is introduced in `atm-core` or storage.
- Existing catalog rows are handled honestly as legacy/unverified until
  re-registered; the migration neither silently labels them `Text` nor claims
  1.4.1 checked-render coverage for them.
- Boundary lint, updated manifests, Rust docs, and error-code inventory pass;
  all test doubles implement the amended sealed port without reimplementing
  upstream behavior.

## Required validation

- `cargo test -p atm-template-sc-compose -p atm-core -p atm-storage-rusqlite`
  with positive and rejection vectors for every production render seam.
- `cargo test -p atm-architecture --test boundary_enforcement` and the
  repository's exact-pin/version checks.
- `just lint` and `just test` on Linux, macOS, and Windows CI.
- A migration/reopen test proving legacy catalog rows are unclassified and a
  newly re-registered row retains its adapter-derived format across process
  restart.
- Retained evidence records: crates.io package/version/checksum lookup,
  sc-compose #448 closure link, exact upstream API call sites, and the final
  CI commit.

## Paths to delete

None. Do not delete historical AN.1 fixture/oracle evidence or legacy catalog
rows. Remove only any temporary 1.4.1 git/path override introduced during a
failed local experiment before opening the implementation PR.

## Non-closure

AN.13 does not make `compose()` itself return a checked type, alter
sc-compose's public API, add non-JSON semantic validation, infer an output
format for legacy rows, synchronize templates across hosts, or add ATM
workflow-specific behavior. Upstream API changes and the direct-library test
belong to sc-compose; ATM consumes the published contract only after its
external gates close.
