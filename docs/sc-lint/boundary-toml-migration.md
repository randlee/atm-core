# Boundary TOML Migration Plan

Related ADR:
- [`./adr/ADR-004-structured-boundary-definitions.md`](./adr/ADR-004-structured-boundary-definitions.md)

Related requirements:
- [`./requirements.md`](./requirements.md)

## Goal

Move canonical boundary data from Markdown-embedded records to standalone TOML
files.

The long-term model should be:

- TOML is the machine-authoritative source of truth
- Markdown remains the human-oriented explanation layer
- `sc-lint-boundary` reads structured boundary data directly, without fenced
  block extraction

## Why Migrate

Current Markdown-embedded boundary records were useful for getting the initial
boundary inventory and enforcement suite moving quickly, but they are not the
best long-term source format for a general-purpose lint tool.

Primary reasons to migrate:

- simpler loader and validation logic
- fewer parser edge cases than fenced YAML inside Markdown
- cleaner schema evolution for `sc-lint-boundary`
- easier fixture authoring for tests
- easier eventual extraction into a separate `sc-lint` repository
- clearer separation between machine policy and human documentation

## Target End State

### Canonical source

Boundary records live in standalone TOML files under owner-package
subdirectories, for example:

```text
boundaries/
  consumer-core/
    mail-store.toml
    identity-registry.toml
  runtime-service/
    server-transport.toml
```

This is the preferred first-rollout layout and matches the recommended
implementation defaults later in this document.

### Human documentation

Markdown boundary docs continue to exist, but they are no longer the
authoritative lint input. They should either:

- summarize the TOML records directly, or
- be generated from TOML later if that becomes worth the effort

## Migration Rules

### During transition

`sc-lint-boundary` should support both:

- existing Markdown-embedded records
- new standalone TOML records

This avoids a flag-day migration.

Default dual-loader rule:

- both formats may exist in one repo during migration
- the same `boundary_id` must not be authoritatively defined in both formats at
  once
- conflicting definitions across formats are errors
- if the same `boundary_id` appears in Markdown and TOML and the normalized
  records differ, that conflicting-definition case is a hard error
- any explicit equivalence mode that permits duplicate-source fixtures is
  test-only and must not be enabled in default lint runs or CI

### For new work

Once TOML loading exists, all new boundary-lint features should be implemented
against TOML-backed data first.

That includes:

- new boundary record fields
- new boundary scan checks
- new rule families that depend on boundary metadata
- the warn/error inventory-parity enforcement model

Markdown support during the transition is compatibility-only, not the place
where new boundary features should first appear.

This includes incoming boundary scan work: once TOML loading exists, any new
boundary scan check should land against TOML-backed boundary data first, even
if Markdown compatibility remains temporarily available for older records.

## Proposed Phases

### Phase 1: Dual Loader

Add loader support for standalone TOML boundary files while preserving current
Markdown support.

Scope:

- define the TOML record schema
- add TOML discovery and parsing
- map TOML records onto the same internal boundary model used today
- keep existing Markdown parsing intact
- add tests proving equivalent record loading from both formats

Deliverable:

- a branch where both boundary sources are accepted
- no behavior regression in existing boundary checks
- duplicate authoritative records across formats fail loudly

### Phase 2: Canonical TOML Rollout

Start migrating the existing boundary inventory into TOML.

Scope:

- create TOML records for current boundaries
- validate that TOML-backed runs match Markdown-backed runs
- keep Markdown docs in sync as human references
- add migration checks for record drift if needed

Deliverable:

- current boundary inventory represented in TOML
- TOML used for new boundary-lint features
- structured planning metadata available for future inventory-parity checks

### Phase 3: Deprecate Markdown as Source

Once all active boundary records are present in TOML and the tooling has been
stable for a full cycle, deprecate Markdown as the authoritative source.

Scope:

- remove Markdown record loading from the analyzer
- keep Markdown docs only as documentation
- simplify test fixtures to TOML-first

Deliverable:

- TOML-only authoritative boundary input
- smaller, cleaner loader and validator code
- all new boundary tests default to TOML fixtures

## Schema Strategy

The migration should preserve the current semantic boundary model as much as
possible. The file format changes first; the rule model does not need to be
reinvented at the same time.

Recommended approach:

- keep the existing internal boundary record struct/model
- deserialize TOML into that existing model
- only add schema changes where TOML materially improves the representation
- for contract-owner records with `implementation.visibility = "trait_only"`,
  absent `implementation.type` and `implementation.module` must be interpreted
  as null rather than represented as empty-string sentinels

Additional recommendation:

- keep planning-aware metadata in TOML as well, rather than splitting boundary
  structure into TOML and planning exceptions into prose or comments

This keeps the migration focused and reduces avoidable rule churn.

## Recommended First-Rollout Defaults

These defaults should be used for the first implementation unless a later
implementation review proves that one of them is unworkable:

- canonical directory:
  - `boundaries/`
- file organization:
  - one file per boundary under owner-package subdirectories, for example:
    - `boundaries/consumer-core/mail-store.toml`
    - `boundaries/runtime-service/server-transport.toml`
- unknown-field policy:
  - strict rejection enabled from day one
- planning metadata location:
  - central registry at `boundaries/planning.toml`, keyed by stable item key
  - current sprint field at `[planning].current_sprint`

These choices optimize for:

- smaller diffs
- simpler fixture authoring
- deterministic discovery
- easier eventual extraction into a standalone `sc-lint` repository

## Testing Plan

At minimum, the migration work should add:

- parser tests for TOML boundary files
- equivalence tests:
  - same boundary record in Markdown source form
  - same boundary record in TOML source form
  - same resulting internal model / same findings
- fixture tests for mixed-mode repos:
  - some records from Markdown
  - some records from TOML
- negative tests for:
  - malformed TOML
  - missing required fields
  - unknown fields if strict mode is adopted
  - duplicate `boundary_id` across TOML files
  - duplicate `boundary_id` across Markdown and TOML during dual-loader mode
  - conflicting same-boundary definitions across formats
  - duplicate item keys in `boundaries/planning.toml`
  - malformed current-sprint setting in planning metadata
- precedence tests proving duplicate-source equivalence mode is unavailable in
  default lint runs and CI
- fixture tests for the recommended `boundaries/<owner-package>/<boundary>.toml`
  layout

Once TOML becomes canonical, new boundary rule tests should use TOML fixtures
first by default.

## Tooling Impact

Expected code changes:

- boundary discovery logic
- boundary parser/loader
- test fixtures and fixture helpers
- doc references to canonical source locations

Expected low-impact areas:

- rule ids
- graph/export schema
- source analysis
- manifest/source enforcement logic

## Remaining Open Decisions

The migration no longer needs a format/location decision before implementation,
but these follow-up decisions should still be reviewed before Phase 3
completes:

- whether Markdown remains hand-maintained or becomes generated
- whether the first-rollout planning registry stays central permanently or
  later moves closer to per-boundary files
- whether later schema evolution needs a dedicated schema-version field inside
  each boundary TOML record

## Relation To Enforcement Planning

The next planned boundary-enforcement feature set depends on this migration:

- inventory-parity warn/error enforcement
- structured future-sprint mappings for planned-but-missing items
- TOML-first boundary additions for future scan checks

That means the migration is not just a format cleanup. It creates the correct
data model for the next enforcement stage.

## Recommendation

Proceed with a dual-loader migration, but treat TOML as the future canonical
format immediately once support lands.

That gives:

- low migration risk
- no forced flag day
- a clean place to add future boundary features
- a better format for eventual `sc-lint` repo extraction
