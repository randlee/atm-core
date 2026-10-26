# `sc-lint` Docs

This folder preserves the design and planning history for `sc-lint`.

> **Current status (2026-10-26):** ATM uses the published `sc-lint` v0.8.0
> tools. The former in-tree `sc-lint-boundary` and `sc-lint-directives`
> implementations were removed during the v0.8.0 adoption; they are not
> vendored in this workspace. Current ATM integration and rule policy are
> configured in [`sc-lint.toml`](../../sc-lint.toml), with the required tool
> version recorded in [`.sc-lint/install.json`](../../.sc-lint/install.json).
> The scaffold and CLI details below are historical snapshots, not current
> implementation or configuration guidance.

Historical and supporting documents:

- [`requirements.md`](./requirements.md) — consumer-neutral requirements for
  boundary-source migration and inventory-parity behavior
- [`mvp.md`](./mvp.md) — MVP design for the initial `sc-lint-boundary`
  analyzer and the paired `sc-lint-attributes` plan
- [`roadmap.md`](./roadmap.md) — historical decisions and rollout sequence
- [`graph-schema.md`](./graph-schema.md) — historical graph/export contract
  and rule-id inventory for the former in-tree analyzer
- [`boundary-enforcement-model.md`](./boundary-enforcement-model.md) — planned
  warn/error escalation model for inventory-parity boundary enforcement
- [`boundary-toml-migration.md`](./boundary-toml-migration.md) — migration plan
  for moving canonical boundary data from Markdown-embedded records to
  standalone TOML

Historical in-tree crate split (before published `sc-lint`):

- `sc-lint-boundary`
  - analyzer CLI + library
  - AST parsing, graph construction, semantic rule evaluation
- `sc-lint-attributes`
  - proc-macro attribute crate
  - intentionally minimal at first
  - exists early so source-level declarations can be added without late
    packaging churn

Historical scaffold status (before published `sc-lint`):

- `sc-lint-attributes`
  - existed as an independently versioned `0.1.0` scaffold
  - provided compile-valid, no-op `#[sc_lint(...)]` support for:
    - `boundary.allow("cycle.type_method_self_loop")`
    - `boundary.allow("cycle.recursive_value_container")`
    - `boundary.internal_only`
- `sc-lint-boundary`
  - existed as an independently versioned `0.1.0` scaffold
  - provided:
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

Current repository integration (published `sc-lint` v0.8.0):

- `just lint` uses the repository's parallel lint runner and the published
  analyzers configured in `sc-lint.toml`.
- CI and release validation use `just lint ci all` for the cross-platform lint
  steps.
- Current boundary and portability rules, including failure policy, are
  declared under `[tool.sc-lint.analyzers.*]` in `sc-lint.toml`. The required
  published tool version is `0.8.0` in `.sc-lint/install.json`.
- No current workspace default-policy file exists at the former in-tree
  `config/defaults.toml` path; use the published tool configuration above.

Rule families planned for that initial scaffold:

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
