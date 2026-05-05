# `sc-lint` Docs

This folder is the home for `sc-lint` design and planning material.

Current contents:

- [`mvp.md`](./mvp.md) — MVP design for the initial `sc-lint-boundary`
  analyzer and the paired `sc-lint-attributes` plan
- [`roadmap.md`](./roadmap.md) — decisions, rollout sequence, and what stays in
  Python vs what moves to Rust

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
    - `boundary.internal_only`
- `sc-lint-boundary`
  - exists now
  - versioned independently at `0.1.0`
  - currently provides:
    - workspace discovery through `cargo_metadata`
    - module-driven source traversal through `syn`
    - graph nodes for crates/modules/types/traits/functions/methods
    - `#[sc_lint(...)]` attribute ingestion for `boundary.allow(...)` and
      `boundary.internal_only`
    - owner-graph cycle classification with:
      - `SCB-CYCLE-001` multi-owner architectural cycle
      - `SCB-CYCLE-002` type/method self-loop
      - `SCB-CYCLE-003` trait-impl self-loop
    - stable text/JSON findings output scaffolding
    - graph JSON export scaffolding

Future documents that should also live here:

- crate layout
- rule inventory
- JSON output schema
- graph export schema
