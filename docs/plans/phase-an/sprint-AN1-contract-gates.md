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

**traceability:** plan-phase-an.md Decisions 2, 3; SHA-drift risk entry.
Requirement IDs to be assigned during plan hardening — do not invent them.

## Deliverables

1. Establish the dedicated `atm-template-sc-compose` adapter boundary and add
   `sc-composer` there as an exact-pinned dependency. In the same PR, add its
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
   boundary manifest must name the same allow/forbid inventory.
2. Confirm the dolt-compatible template hash is public `sc-composer` API. If
   it is internal, land the upstream export change in `randlee/sc-compose`
   first and record the released upstream version this sprint consumes. atm
   must not reimplement the hash.
3. Golden-vector oracle: a fixture set of template files — including CRLF,
   LF, BOM-prefixed, and no-trailing-newline variants — whose SHAs are
   recorded from synaptic-canvas-dolt's actual output, with a test asserting
   byte-equality through the dedicated `atm-template-sc-compose` adapter API. The input
   contract is **raw file bytes; atm performs no normalization**.
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
}

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

4. Frontmatter extraction and include-detection seam via the dedicated
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

5. FTS5 availability gate test in `atm-storage-rusqlite`: create an FTS5
   virtual table in a temp DB and fail loudly if the bundled SQLite build
   lacks it.
6. Fixture capture for AN.8: one real task-assignment template, one real
   QA-report template, and one existing agent-written Python tmp-file parser,
   stored under `docs/plans/phase-an/fixtures/` verbatim.

## Acceptance criteria

- Golden vectors match dolt-recorded SHAs byte-for-byte on macOS, Linux, and
  Windows CI lanes (CRLF checkout variants included).
- The hash and frontmatter APIs consumed are public `sc-composer` items at
  the pinned version; no digest or YAML parsing logic exists in atm code. The
  only upstream call sites are in `atm-template-sc-compose`.
- The FTS5 gate test passes on all CI platforms.
- `extract_frontmatter` round-trips both captured real templates.
- Include-analysis fixtures prove every supported dependency directive
  populates `include_references` and directive-looking literal text does not;
  an unknown/unclassifiable upstream directive fails closed.
- No file created or modified by this sprint appears in the frozen AM removal
  ledger.
- Boundary lint verifies the only production `TemplateComposer` implementation
  is the recorded adapter and the only production wiring is the recorded
  replacement composition path. A composition test builds the replacement
  bootstrap assembly with the adapter and proves core receives only the
  `TemplateComposer` port; all forbidden-edge assertions above run as the
  architecture merge gate.

## Required validation

- cargo test/format/lint suite
- cross-platform CI including a Windows CRLF-checkout lane
- AM-ledger boundary check on the sprint's changed-file list

## Non-closure

No database schema, no storage behavior, no CLI surface, and no send/read
changes land in this sprint. This sprint defines no search implementation;
the reusable storage search capability is AN.5.
