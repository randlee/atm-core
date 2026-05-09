# `sc-lint` Requirements

These requirements define the planned behavior for the boundary-definition and
inventory-parity work in the `sc-lint` tool family.

## Canonical Boundary Source

- `REQ-SCB-001`
  Canonical machine-readable boundary definitions must live in standalone TOML
  files under the `boundaries/` directory.

## Dual-Loader Transition

- `REQ-SCB-002`
  During migration, the loader must accept both Markdown-embedded boundary
  records and TOML boundary records while mapping both sources into one shared
  internal boundary model.

- `REQ-SCB-003`
  During the dual-loader phase, duplicate `boundary_id` authority across
  sources is a hard error by default.

- `REQ-SCB-004`
  During the dual-loader phase, conflicting definitions for the same
  `boundary_id` across Markdown and TOML are a hard error.

- `REQ-SCB-005`
  Any migration equivalence mode that permits duplicate-source fixtures must be
  test-only and disabled in default developer runs and CI.

## TOML-First Evolution

- `REQ-SCB-006`
  Once TOML loading exists, new boundary-enforcement features that depend on
  boundary metadata must be implemented against TOML-backed data first.

## Inventory-Parity Enforcement

- `REQ-SCB-007`
  Inventory-parity checks must compare structured boundary requirements against
  the code graph at item-key granularity.

- `REQ-SCB-008`
  A missing documented item with no valid planning mapping must fail with
  `SCB-INVENTORY-001`.

- `REQ-SCB-009`
  A missing documented item scheduled in a future sprint may warn with
  `SCB-INVENTORY-002`.

- `REQ-SCB-010`
  A missing documented item whose scheduled sprint is current or past must fail
  with `SCB-INVENTORY-003`.

- `REQ-SCB-011`
  Inventory-parity warning eligibility must come only from structured planning
  metadata, not prose, comments, or freeform allowlists.

- `REQ-SCB-012`
  The default planning metadata source for inventory parity is
  `boundaries/planning.toml`.

- `REQ-SCB-013`
  `boundaries/planning.toml` must define `[planning].current_sprint`, and
  current-sprint parsing failure must cause planned-but-missing items to fail
  rather than warn.

- `REQ-SCB-014`
  Sprint comparison for inventory parity must use parsed ordering, not lexical
  string comparison.
