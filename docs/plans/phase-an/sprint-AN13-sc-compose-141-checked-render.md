---
title: AN.13 sc-compose 1.4.1 Checked-Render Catalog Format Contract
status: complete
branch: feature/an13-sc-compose-141-checked-render
target: integrate/phase-an
worktree: ../atm-core-worktrees/feature/an13-sc-composer-141-upgrade
external_blockers: []
---

# AN.13 — sc-compose 1.4.1 Checked-Render Catalog Format Contract

**recommended_agent:** arch-ctm/deep-reasoning (sealed renderer-port and
durable output-format contract).
**must_follow:** AN.10 merged, because AN.10 owns the atomic decomposed
admission path and `TemplateCatalogStore` contract this sprint extends. Before
every dev/fix round, merge the pushed AN.10 integration tip. In addition, this
sprint was **blocked** until all three external conditions held. They are now
satisfied and retained in [the AN.13 release evidence](./sc-compose-141-evidence.md):

1. crates.io publishes `sc-sha` **1.4.1**, `sc-composer` **1.4.1**, and
   `sc-compose` **1.4.1**, and the published `sc-composer` release exports
   `check_rendered_output`, `CheckedOutput`, and `OutputFormat`; and
2. [sc-compose #448](https://github.com/randlee/sc-compose/issues/448) is
   closed with direct-library regression coverage for the documented
   `compose` → `check_rendered_output` checked-emission sequence.

Do not use an unpublished git revision, a local path override, or a version
range to bypass these gates. The adapter uses the exact released `=1.4.1` pin
to call `OutputFormat::from_template_path`; checked emission remains AN.14.

**unblocks:** AN.14, the runtime checked-emission upgrade. It does not unblock
the workflow-metadata extension.
**parallel_safe:** none. AN.13 changes the same catalog/admission contract
AN.10 makes atomic and must be reviewed as one durable-format boundary.

**traceability:** Phase AN Decisions 2, 3, 5, and 8; ADR-036; the existing
exact-pin policy in AN.1; `docs/atm-adapter-notes.md` in sc-compose; and
sc-compose #448.

## Deliverables

1. Extend the core-owned `TemplateComposer` port, its sealed production
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

2. Add the additive catalog migration and DTO/trait/boundary records needed by
   Deliverable 1. New templates always persist `Text` or `Json`. Pre-AN.13
   catalog rows have no trustworthy filename/format identity, so their
   migration state is an explicit legacy/unverified state: they remain
   readable under the pre-AN.13 compatibility behavior but cannot be claimed
   as checked-render proof. The migration must not guess from template body or
   metadata. Document the operator migration/re-registration path that turns a
   legacy row into a newly admitted, format-classified row.

3. Update the boundary-enforcement expectations and relevant migration/API
   documentation together. `atm-template-sc-compose` remains the sole
   authorized implementation and upstream dependency owner; `atm-core`,
   `atm-storage`, `atm-storage-rusqlite`, CLI, daemon, runtime, and HTTP crates
   remain forbidden from importing `sc_composer` or `sc_sha`.

## Acceptance criteria

- sc-compose #448 is closed, and its merged upstream test demonstrates the
  direct-library malformed-JSON rejection path used by adapters. AN.13 records
  the closed issue URL and upstream release version in retained evidence before
  changing the durable contract.
- File extension classification occurs solely through the released upstream API
  at the adapter boundary. Stored render-on-read uses the persisted format; no
  path/body/metadata inference is introduced in `atm-core` or storage.
- Existing catalog rows are handled honestly as legacy/unverified until
  re-registered; the migration neither silently labels them `Text` nor claims
  1.4.1 checked-render coverage for them.
- Boundary lint, updated manifests, migration/API docs, and all amended test
  doubles pass;
  all test doubles implement the amended sealed port without reimplementing
  upstream behavior.

## Required validation

- `cargo test -p atm-template-sc-compose -p atm-core -p atm-storage-rusqlite`
  with file-admission, catalog persistence, legacy migration, and restart
  vectors for the durable format contract.
- `cargo test -p atm-architecture --test boundary_enforcement` and the
  repository's boundary-manifest checks.
- `just lint` and `just test` on Linux, macOS, and Windows CI.
- A migration/reopen test proving legacy catalog rows are unclassified and a
  newly re-registered row retains its adapter-derived format across process
  restart.
- Retained evidence records: crates.io package/version/checksum lookup,
  sc-compose #448 closure link, exact upstream API call sites, and the final
  CI commit.

## Paths to delete

None. Do not delete historical AN.1 fixture/oracle evidence or legacy catalog
rows.

## Non-closure

AN.13 alters only the adapter-owned exact `sc-composer` pin to `=1.4.1`,
because that released version supplies the `OutputFormat::from_template_path`
API used for durable admission classification. It does not invoke checked
rendering, make `compose()` itself return a checked type, infer an output
format for legacy rows, synchronize templates across hosts, or add
ATM workflow-specific behavior. AN.14 owns every runtime checked-emission
route. Upstream API changes and the direct-library test belong to sc-compose.
