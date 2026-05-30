# `sc-lint` Docs

This folder is the home for `sc-lint` design and planning material.

Current contents:

- [`requirements.md`](./requirements.md) — consumer-neutral requirements for
  boundary-source migration and inventory-parity behavior
- [`mvp.md`](./mvp.md) — MVP design for the initial `sc-lint-boundary`
  analyzer and the paired `sc-lint-attributes` plan
- [`roadmap.md`](./roadmap.md) — decisions, rollout sequence, and what stays in
  Python vs what moves to Rust
- [`graph-schema.md`](./graph-schema.md) — current graph/export contract and
  rule-id inventory
- [`boundary-enforcement-model.md`](./boundary-enforcement-model.md) — planned
  warn/error escalation model for inventory-parity boundary enforcement
- [`boundary-toml-migration.md`](./boundary-toml-migration.md) — migration plan
  for moving canonical boundary data from Markdown-embedded records to
  standalone TOML

Current intended crate split:

- `sc-lint-boundary`
  - analyzer CLI + library
  - AST parsing, graph construction, semantic rule evaluation
- `sc-lint-attributes`
  - proc-macro attribute crate
  - intentionally minimal at first
  - exists early so source-level declarations can be added without late
    packaging churn

Current scaffold status:

- `sc-lint-attributes`
  - exists now
  - versioned independently at `0.1.0`
  - currently provides compile-valid, no-op `#[sc_lint(...)]` support for:
    - `boundary.allow("cycle.type_method_self_loop")`
    - `boundary.allow("cycle.recursive_value_container")`
    - `boundary.internal_only`
- `sc-lint-boundary`
  - exists now
  - versioned independently at `0.1.0`
  - currently provides:
    - workspace discovery through `cargo_metadata`
    - module-driven source traversal through `syn`
    - graph nodes for:
      - crates
      - modules
      - types
      - impls
      - variants
      - fields
      - traits
      - trait references
      - functions
      - methods
    - `#[sc_lint(...)]` attribute ingestion for `boundary.allow(...)` and
      `boundary.internal_only`
    - owner-graph cycle classification with:
      - `SCB-CYCLE-001` multi-owner architectural cycle
      - `SCB-CYCLE-002` type/method self-loop
      - `SCB-CYCLE-003` trait-impl self-loop
    - built-in default trait-self-loop policy from:
      - `crates/sc-lint-boundary/config/defaults.toml`
    - boundary enforcement with:
      - `SCB-BOUNDARY-001` internal_only visibility violation
      - `SCB-BOUNDARY-002` internal_only external reference
      - `SCB-BOUNDARY-003` forbid_external_impls violation
    - portability enforcement with:
      - `PORT-001` hardcoded Unix-only absolute paths in test code
      - `PORT-002` direct `dirs::home_dir()` without configured override check
      - `PORT-003` `std::env::set_var()` in test code
    - stable text/JSON findings output
    - graph export in:
      - JSON
      - Turtle

Current repo integration status:

- `just lint sc-boundary`
  - exists now as a separate preliminary/manual target
  - is intentionally not part of default `just lint` yet
  - default `just lint` integration remains deferred until the manual/preliminary
    gate criteria are explicitly approved
- `just lint sc-portability`
  - exists now as a separate preliminary/manual target
  - is intentionally not part of default `just lint` yet
  - default `just lint` integration remains deferred until the manual/preliminary
    gate criteria are explicitly approved

Planned rule families not implemented yet:

- `SCB-INVENTORY-001`
- `SCB-INVENTORY-002`
- `SCB-INVENTORY-003`

Planned canonical boundary layout:

```text
boundaries/
  consumer-core/
    mail-store.toml
    identity-registry.toml
  runtime-service/
    server-transport.toml
  planning.toml
```

Future documents that should also live here:

- crate layout
- rule inventory
- deeper RDF/Oxygraph integration notes

Related architecture decision:

- [`./adr/ADR-004-structured-boundary-definitions.md`](./adr/ADR-004-structured-boundary-definitions.md)
  — canonical TOML boundary source plus planning-aware inventory-parity
  enforcement
