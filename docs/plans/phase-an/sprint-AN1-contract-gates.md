---
title: AN.1 Contract Gates — sc-composer Seam, dolt SHA Oracle, FTS5 Gate
status: draft
branch: feature/pan-s1-contract-gates
worktree: ../atm-core-worktrees/feature/pan-s1-contract-gates
target: integrate/phase-an
---

# AN.1 — Contract Gates: sc-composer Seam, dolt SHA Oracle, FTS5 Gate

**recommended_agent:** Cipher-311d/fast (bounded oracle/fixture work; escalate
to arch-ctm/deep-reasoning only if the upstream hash API requires design).
**must_follow:** none within AN. Phase entry gate applies (AM.2 merged before
any AN sprint that would touch limits; AN.1 touches none).
**unblocks:** AN.2 (and transitively all of AN).
**parallel_safe:** with late Phase AM sprints — AN.1 adds a new dependency,
fixtures, and gate tests only; every file it creates or edits must be checked
against the frozen AM removal ledger before dispatch.

**traceability:** plan-phase-an.md Decisions 2, 3, 5, 8; ADR-036's Phase AN
extension; SHA-drift risk entry. Requirement IDs to be assigned during plan
hardening — do not invent them.

## Deliverables

1. Before AN.2 can start, record and accept ADR-036's Phase AN extension: it
   is the ADR-018 §3 follow-up authorizing exactly `TemplateCatalogStore` and
   `MessageSearchStore` as optional capability traits four and five, preserves
   leaf `atm-storage` DTO ownership, and records the template adapter,
   cross-host plain-text, and include-fallback policies. No AN.2 schema or
   AN.5 capability code may land without that accepted ADR record.
2. Establish the dedicated `atm-template-sc-compose` adapter boundary. Until
   the required public `sc-compose` and `sc-sha` APIs are released, this sprint
   lands a fixture-only stub in that crate: it may prove ATM port wiring and
   fail-closed policy, but must not hash, parse, scan directives, resolve
   paths, or claim golden-vector/containment proof. The first real-adapter PR
   replaces that stub with an exact-pinned `sc-composer` dependency; no caller
   changes are allowed in that replacement. `sc-sha` is limited to per-file
   and recursive hashing; ATM owns the future template graph manifest and
   associated storage semantics. In the same PR, add its
   package/boundary inventory and the `atm-core` `TemplateComposer` port
   record, name `atm-daemon-bootstrap` replacement composition (via
   `atm-runtime` assembly) as the one authorized production wiring site, and
   register the adapter implementation plus any test double under ADR-001's
   boundary-lint allowlists. Document the pin policy in the adapter manifest:
   any version bump requires re-running the golden-vector suite (Deliverable
   3) in the same PR. `atm-storage`, `atm-core`, the CLI, and
   `atm-http-runtime` do not depend directly on `sc-composer`. This is an
   executable merge gate, not prose: extend
   `crates/atm-architecture/tests/boundary_enforcement.rs`'s
   `EXPECTED_FORBIDDEN_EDGES` and its
   `assert_forbidden_edge_absent` coverage to reject these direct workspace
   edges to `atm-template-sc-compose`: `atm-core`, `atm-storage`,
   `atm-storage-rusqlite`, `atm`, `atm-daemon`, `atm-runtime`, and
   `atm-http-runtime`. The only authorized production dependent is
   `atm-daemon-bootstrap`, which constructs the adapter and injects its
   `TemplateComposer` port through the `atm-runtime` assembly. The adapter
   boundary manifest must name the same allow/forbid inventory. Create
   `boundaries/atm-template-sc-compose/sc-composer.toml` and register that
   exact path in `guarded_boundary_files()` in
   `crates/atm-architecture/tests/boundary_enforcement.rs`; the generic
   expected-edge/manifest cross-check must fail if this new manifest is absent.
3. Confirm the dolt-compatible template hash **and the directive-kind-
   classified, span-annotated include/import/from-import inspection API** are
   public `sc-composer` APIs. If either is internal or incomplete, land the
   upstream export change in `randlee/sc-compose` first and record the
   released upstream version this sprint consumes. ATM must not reimplement
   the hash or degrade include inspection to a substring heuristic.
4. Golden-vector oracle: a fixture set of template files — including CRLF,
   LF, BOM-prefixed, and no-trailing-newline variants — whose SHAs are
   recorded from synaptic-canvas-dolt's actual output, with a test asserting
   byte-equality through the dedicated `atm-template-sc-compose` adapter API.
   The adapter strictly decodes UTF-8 and normalizes `CRLF` and lone `CR` to
   `LF` before hashing, so equivalent text has one platform-independent
   identity on Windows, macOS, and Linux. ATM retains the original source
   bytes for rendering/audit; it does not implement the normalization itself.
   `TemplateSha` and `TemplateFrontmatter` are leaf storage-contract DTOs;
   the adapter produces them through the core-owned renderer port rather than
   by giving storage or transports access to the upstream library:

```rust
/// Leaf storage-contract identifier: lowercase hex, exact dolt encoding.
pub struct TemplateSha(String);

/// Core-owned port. `atm-template-sc-compose` delegates to sc-composer; ATM
/// never computes digests or parses frontmatter itself.
pub trait TemplateComposer {
    fn inspect(&self, raw_file_bytes: &[u8]) -> Result<TemplateInspection, AtmError>;
    fn render_within_root(
        &self,
        template: &TemplateSource,
        vars: &serde_json::Map<String, serde_json::Value>,
        root: &TemplateRoot,
    ) -> Result<RenderedBody, AtmError>;
    fn render_without_includes(
        &self,
        source: &TemplateSource,
        vars: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<RenderedBody, AtmError>;
}

pub struct TemplateSource { pub raw_file_bytes: Vec<u8> }
pub struct TemplateRoot { pub canonical_path: PathBuf }

pub struct TemplateInspection {
    pub sha: TemplateSha,
    pub frontmatter: TemplateFrontmatter,
    pub include_references: Vec<TemplateReference>,
}

/// Every upstream template-dependency directive is represented here, including
/// static and expression-valued includes/imports. Empty means the pinned
/// upstream parser proved the raw file has no dependency directive.
pub struct TemplateReference {
    pub directive: TemplateReferenceKind,
    pub source_span: SourceSpan,
}

pub struct SourceSpan {
    pub byte_start: usize,
    pub byte_end: usize,
}

pub enum TemplateReferenceKind {
    Include,
    Import,
    FromImport,
}
```

5. Frontmatter extraction and include-detection seam via the dedicated
   `atm-template-sc-compose` adapter,
   producing the structure
   AN.2 persists as `schema_json`:

```rust
pub struct TemplateFrontmatter {
    pub required_variables: Vec<String>,
    pub defaults: serde_json::Map<String, serde_json::Value>,
    pub metadata: serde_json::Map<String, serde_json::Value>, // incl. type key
}

pub fn extract_frontmatter(raw_file_bytes: &[u8])
    -> Result<TemplateFrontmatter, AtmError>;
```

   `TemplateComposer::inspect` uses the pinned public upstream parser over
   the raw bytes; an ATM substring heuristic is forbidden. Fixture coverage
   must exercise every upstream dependency-form (`include`, `import`, and
   `from … import`) with static and expression-valued targets, plus ordinary
   text that resembles a directive. Thus an empty `include_references` is a
   parser-backed assertion, not a possible false negative. If the parser
   cannot classify a source form after an upstream upgrade, inspection fails
   closed with a typed diagnostic rather than admitting a decomposed message.
   `render_within_root` uses the same pinned upstream resolver: every loaded
   include/import target is canonicalized and proven to remain below
   `TemplateRoot`; absolute targets, lexical `..` escape, and symlink escape
   fail before render. ATM must not replace this proof with a local path-prefix
   or substring check.
   `render_without_includes` is a distinct stored-template path, never a
   generic rootless render. It first runs `inspect`; when
   `include_references` is non-empty it returns
   `DECOMPOSED_TEMPLATE_INCLUDE_FORBIDDEN` **before any resolver, loader, or
   filesystem callback is reachable**. Only a parser-proven empty reference
   set may render the raw source with includes disabled. AN.4 uses this method
   exclusively for BLOB-backed decomposed render-on-read; AN.3 alone uses
   `render_within_root` for the include-fallback verification render.

6. FTS5 availability gate test in `atm-storage-rusqlite`: create an FTS5
   virtual table in a temp DB and fail loudly if the bundled SQLite build
   lacks it.
7. Fixture capture for AN.8: one real task-assignment template, one real
   QA-report template, and one existing agent-written Python tmp-file parser,
   stored under `docs/plans/phase-an/fixtures/` verbatim.

## Acceptance criteria

- The fixture adapter proves only the sealed `TemplateComposer` port wiring
  and the fail-closed stored-render policy: explicitly registered inspection
  results drive `render_without_includes`, and a registered dependency result
  returns `DECOMPOSED_TEMPLATE_INCLUDE_FORBIDDEN` before any loader-backed
  render is attempted.
- The FTS5 gate test passes on all CI platforms.
- No file created or modified by this sprint appears in the frozen AM removal
  ledger.
- Boundary lint verifies the fixture adapter is the recorded implementation
  of the sealed port and that only `atm-daemon-bootstrap` is an allowed
  production dependent; all forbidden-edge assertions run as the architecture
  merge gate.

### Exact-pin replacement acceptance criteria

The following acceptance criteria now apply to the caller-preserving exact-pin
replacement landed by AN.3. The classified directive-inspection portion remains
the only deferred item tracked by
`.triage/phase-an/findings/AN1-FIXTURE-STUB-REPLACEMENT-001.ttl`:

- Golden vectors match the normalized-text SHA on macOS, Linux, and Windows
  CI lanes: LF and CRLF representations of equivalent text produce the same
  `TemplateSha`; BOM and final-newline semantics remain explicit.
- The pinned upstream APIs provide `extract_frontmatter`, directive-kind and
  span-aware include/import/from-import inspection, and root-constrained
  rendering. Fixtures cover all dependency forms, directive-looking literal
  text, and absolute, `..`, and symlink escapes. As of the published 1.4.0
  line, identity, frontmatter, and containment are available; classified
  directive inspection is still not public and remains the only deferred
  portion of `AN1-FIXTURE-STUB-REPLACEMENT-001`.
- `dolt-template-sha-vectors.json` is wired into that adapter's golden-vector
  test and is an executable oracle for the released identity contract.
- The replacement adapter, not ATM code, performs hashing, frontmatter
  extraction, directive inspection, and containment proof through the exact
  upstream pin.

## Required validation

- cargo test/format/lint suite for the fixture adapter, port, architecture,
  and FTS5 gate
- cross-platform CI for the fixture adapter and boundary gate, including the
  Windows CRLF golden-vector lane
- AM-ledger boundary check on the sprint's changed-file list

## Non-closure

No database schema, no storage behavior, no CLI surface, and no send/read
changes land in this sprint. This sprint defines no search implementation;
the reusable storage search capability is AN.5.

The adapter is production template support for the released identity,
frontmatter, rendering, and root-containment APIs. Only classified directive
kind/span inspection remains fixture-backed because its upstream public API is
not yet released. The adapter exists so the ATM port, bootstrap ownership, and
architecture enforcement remain complete without duplicating upstream
functionality locally.

The deferred replacement is explicitly tracked in
`.triage/phase-an/findings/AN1-FIXTURE-STUB-REPLACEMENT-001.ttl`. It is not a
claim that the fixture is production-capable: it remains deferred and
non-dispatchable until the required public `sc-compose` and `sc-sha` APIs are
published, then must be completed as a caller-preserving exact-pin swap.
