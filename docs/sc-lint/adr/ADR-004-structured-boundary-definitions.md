# ADR-004 — Structured Boundary Definitions And Planning-Aware Inventory Parity

| Field | Value |
|---|---|
| ID | ADR-004 |
| Status | **Accepted** |
| Date | 2026-05-07 |
| Deciders | Rand Lee |
| Relates to | REQ-SCB-001, REQ-SCB-007, REQ-SCB-012 |
| Supersedes | — |

---

## Context

The current boundary source format is Markdown with embedded structured
records. That was good enough to get schema checks, dependency-edge checks, and
reference checks moving quickly, but it is not the best long-term format for
general-purpose `sc-lint` tooling.

Two new enforcement needs now exist:

1. canonical machine-readable boundary definitions suitable for future
   extraction into the `sc-lint` tool family
2. inventory-parity checks that compare documented boundary requirements
   against the code graph and distinguish:
   - planned future work
   - overdue planned work
   - unplanned architectural drift

The current Markdown-embedded format makes both goals harder because it mixes:

- human explanation
- machine policy
- future planning context

in one document source.

## Decision Drivers

- canonical machine-readable boundary data should be simple to parse and
  validate
- future boundary-enforcement features should be data-driven, not prose-driven
- planned future work must stay visible without becoming an indefinite warning
  loophole
- new architectural drift must fail immediately
- the migration should avoid a flag day

## Options Considered

### Keep Markdown as the long-term authoritative source

Rejected.

This keeps the current mixed human/machine format and makes future structured
planning-aware enforcement harder than necessary.

### Switch to TOML in one flag-day change

Rejected.

The migration risk is higher than necessary. A dual-loader phase is safer.

### Use freeform prose or comments for planned future exceptions

Rejected.

That would turn warnings into a weak suppression system and break deterministic
lint behavior.

## Decision

### 1. TOML becomes the canonical machine-readable boundary source

Boundary definitions will migrate from Markdown-embedded records to standalone
TOML records.

Markdown may remain as:

- human explanation
- generated or hand-maintained summary

but it will no longer be the long-term authoritative lint input.

### 2. Migration uses a dual-loader transition

The transition proceeds in phases:

1. support both Markdown-embedded records and TOML records
2. migrate existing records into TOML
3. remove Markdown record loading after the migration is complete

### 3. New boundary-enforcement features are TOML-first

Once TOML loading exists, any new boundary-lint feature that depends on
boundary metadata must be implemented against TOML-backed data first.

Markdown compatibility may remain during transition, but it is compatibility
only.

### 4. Inventory parity uses planning-aware warn/error enforcement

Boundary lint will compare documented required items against the code graph and
classify missing items using structured planning metadata:

- future-scheduled item: warning
- overdue scheduled item: error
- unscheduled item: error

The warning model is not freeform suppression. It is structured, traceable, and
auto-escalating.

### 5. Planning metadata uses a concrete default location

The default planning metadata source for inventory-parity evaluation is:

- `boundaries/planning.toml`

The first-rollout planning metadata shape uses:

- `[planning].current_sprint`
- `[planned_items."<item-key>"]`

### 6. Duplicate authoritative records across formats are errors

During the dual-loader phase, the same boundary record must not be defined
authoritatively in both Markdown and TOML at once unless the tooling has an
explicit migration mode proving equivalence.

Default rule:

- duplicate `boundary_id` across sources is an error
- conflicting definitions across sources are an error

This prevents silent drift between two sources of truth.

## Consequences

### Positive

- simpler long-term parser and validator design
- better fit for future `sc-lint` extraction
- clearer separation between architecture docs and machine policy
- planned future work remains visible and enforceable
- new drift fails immediately

### Negative

- one more migration step before the boundary toolchain is fully settled
- temporary dual-source complexity
- additional planning metadata will need structure and validation

## Follow-Up Work

| Action | Owner | Gate |
|---|---|---|
| Implement the TOML dual-loader | `sc-lint-boundary` maintainers | Before new boundary metadata features land |
| Define the canonical TOML schema shape | `sc-lint-boundary` maintainers | Before broad record migration begins |
| Define structured planning metadata for inventory parity | `sc-lint-boundary` maintainers | Before `SCB-INVENTORY-*` implementation |
| Add warning/error escalation behavior to boundary lint | `sc-lint-boundary` maintainers | After TOML loading is in place |
| Remove Markdown record loading after migration completes | `sc-lint-boundary` maintainers | After a full stable cycle with TOML-authoritative records |

*ADR-004 | sc-lint | 2026-05-07*
