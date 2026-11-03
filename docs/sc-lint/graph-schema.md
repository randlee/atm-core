# `sc-lint` Graph Schema

> **Historical snapshot:** this document describes the `0.1.0` graph exporter
> from the former in-tree `sc-lint-boundary` scaffold. That crate was removed
> during ATM's adoption of published `sc-lint` v0.8.0. This is not the current
> CLI or export contract. Current analyzer integration and rule configuration
> live in [`sc-lint.toml`](../../sc-lint.toml); the required published tool
> version is in [`.sc-lint/install.json`](../../.sc-lint/install.json).

The sections below are retained as a historical record of the internal
scaffold's graph format.

## Versioning

- tool: `sc-lint-boundary`
- schema version at the time: `0.1.0`

Both graph JSON and findings JSON include:

- `tool`
- `version`
- `schema_version`

The schema version is the compatibility key for downstream graph consumers.

## Graph Export Formats

Supported export formats:

- `json`
- `turtle`

Former in-tree CLI:

```text
sc-lint-boundary export-graph --root <repo> --format json
sc-lint-boundary export-graph --root <repo> --format turtle
```

## Node Model

Node kinds at the time:

- `crate`
- `module`
- `type`
- `variant`
- `field`
- `trait`
- `trait_ref`
- `function`
- `method`
- `impl`

Shared node fields at the time:

- `id`
- `kind`
- `label`
- `visibility`
- `package`
- `target`
- `manifest_path`
- `source_path`
- `module_path`
- `impl_kind`
- `impl_trait`
- `attributes`

Notes:

- `trait_ref` is used when an impl references a trait path that does not map to
  a local trait node already present in the graph.
- `visibility` is a normalized string:
  - `private`
  - `public`
  - `crate`
  - `restricted`

## Edge Model

Edge kinds at the time:

- `contains`
- `declares`
- `targets`
- `implements`
- `references`
- `references_type`
- `references_expr`

Notes:

- `references` is the compatibility aggregate edge.
- `references_type` and `references_expr` are the preferred semantic edges for
  new rule work.
- `targets` connects an `impl` node to the type it implements for.
- `implements` connects an `impl` node to a `trait` or `trait_ref` node.

## Findings Model

Finding fields at the time:

- `rule_id`
- `kind`
- `message`
- `owner_ids`
- `node_ids`

Implemented rule ids at the time:

- `SCB-CYCLE-001`
- `SCB-CYCLE-002`
- `SCB-CYCLE-003`
- `SCB-BOUNDARY-001`
- `SCB-BOUNDARY-002`
- `SCB-BOUNDARY-003`
- `PORT-001`
- `PORT-002`
- `PORT-003`

## Source-Level Allowances

Implemented source-level allowances at the time:

- `boundary.allow("cycle.type_method_self_loop")`
- `boundary.allow("cycle.recursive_value_container")`

`cycle.recursive_value_container` is intended for deliberate recursive
container/value models where every owner participating in the recursive cycle
opts in explicitly.

## Default Trait Policy

The former in-tree analyzer read its embedded default policy from:

- `crates/sc-lint-boundary/config/defaults.toml` (deleted with the scaffold)

Current rule policy is configured for the published analyzer in the repository
configuration, [`sc-lint.toml`](../../sc-lint.toml), under
`[tool.sc-lint.analyzers.boundary]` and
`[tool.sc-lint.analyzers.portability]`.

At the time, trait self-loop filtering was configured through:

- `trait_self_loop.ignored_trait_paths`
- `trait_self_loop.ignored_trait_names`

This keeps common ecosystem trait families configurable without baking
repo-specific names into analyzer code.

## Stability Notes

Stable enough to build against at the time:

- `tool`
- `schema_version`
- node/edge top-level field names
- rule ids at the time

Expected to evolve:

- exact node kind inventory
- exact edge kind inventory
- which advisory findings are emitted by default
- Turtle vocabulary expansion
