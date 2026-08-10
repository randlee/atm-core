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

1. Add `sc-composer` as an exact-pinned dependency of one dedicated
   `sc-composer` adapter crate. Document the pin policy in that crate's
   manifest comment: any version bump requires re-running the golden-vector
   suite (Deliverable 3) in the same PR. `atm-storage`, `atm-core`, the CLI,
   and `atm-http-runtime` do not depend directly on `sc-composer`.
2. Confirm the dolt-compatible template hash is public `sc-composer` API. If
   it is internal, land the upstream export change in `randlee/sc-compose`
   first and record the released upstream version this sprint consumes. atm
   must not reimplement the hash.
3. Golden-vector oracle: a fixture set of template files — including CRLF,
   LF, BOM-prefixed, and no-trailing-newline variants — whose SHAs are
   recorded from synaptic-canvas-dolt's actual output, with a test asserting
   byte-equality through the dedicated `sc-composer` adapter API. The input
   contract is **raw file bytes; atm performs no normalization**.
   `TemplateSha` and `TemplateFrontmatter` are leaf storage-contract DTOs;
   the adapter produces them through the core-owned renderer port rather than
   by giving storage or transports access to the upstream library:

```rust
/// Leaf storage-contract identifier: lowercase hex, exact dolt encoding.
pub struct TemplateSha(String);

/// Core-owned port. Its dedicated adapter delegates to sc-composer; ATM
/// never computes digests or parses frontmatter itself.
pub trait TemplateComposer {
    fn inspect(&self, raw_file_bytes: &[u8]) -> Result<TemplateInspection, AtmError>;
}

pub struct TemplateInspection {
    pub sha: TemplateSha,
    pub frontmatter: TemplateFrontmatter,
}
```

4. Frontmatter extraction seam via the dedicated `sc-composer` adapter,
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
  only upstream call sites are in the dedicated adapter crate.
- The FTS5 gate test passes on all CI platforms.
- `extract_frontmatter` round-trips both captured real templates.
- No file created or modified by this sprint appears in the frozen AM removal
  ledger.

## Required validation

- cargo test/format/lint suite
- cross-platform CI including a Windows CRLF-checkout lane
- AM-ledger boundary check on the sprint's changed-file list

## Non-closure

No database schema, no storage behavior, no CLI surface, and no send/read
changes land in this sprint. This sprint defines no search implementation;
the reusable storage search capability is AN.5.
