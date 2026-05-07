# ADR-004 — Structured Boundary Definitions And Planning-Aware Inventory Parity

| Field | Value |
|---|---|
| ID | ADR-004 |
| Status | **Accepted** |
| Date | 2026-05-07 |
| Deciders | arch-inj, team-lead, arch-ctm |
| Relates to | REQ-SCB-001 through REQ-SCB-014 |

---

## Context

`sc-lint-boundary` now has enough AST and graph machinery to enforce concrete
structural rules, but the current Markdown-embedded boundary-record model is a
poor long-term source for:

- inventory-parity checks such as "documented item exists in code"
- planning-aware warn/error escalation for future-sprint gaps
- extraction of `sc-lint` into its own repository

The tool needs one machine-authoritative source for:

- boundary definitions
- planning metadata for missing documented items
- deterministic warning-to-error escalation rules

## Decision

`sc-lint` adopts the following model:

- canonical machine-readable boundary definitions live in standalone TOML files
  under `boundaries/`
- planning metadata for inventory-parity enforcement lives in
  `boundaries/planning.toml`
- inventory-parity checks compare structured boundary items against the code
  graph at item-key granularity
- missing documented items may warn only when they have a valid structured
  future-sprint mapping
- unplanned or overdue missing documented items fail as errors

## Consequences

### Positive

- boundary inventories become directly parseable without Markdown fenced-block
  extraction
- warn/error behavior becomes deterministic rather than prose-driven
- the tool can fail new architectural drift immediately while still surfacing
  planned future work
- future `sc-lint` extraction becomes simpler because the canonical data model
  is already repo-neutral

### Negative

- the dual-loader migration must exist for one transition period
- consumer repositories must maintain a structured `boundaries/planning.toml`
  file once inventory-parity enforcement begins

## Required Follow-Up

- keep duplicate-source equivalence mode test-only and disabled in normal lint
  runs and CI
- implement `SCB-INVENTORY-001`, `SCB-INVENTORY-002`, and
  `SCB-INVENTORY-003` against TOML-backed boundary data
- make `[planning].current_sprint` in `boundaries/planning.toml` the
  authoritative current-sprint source for warn/error escalation

*ADR-004 | sc-lint | 2026-05-07*
