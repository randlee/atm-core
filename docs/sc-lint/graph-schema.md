# `sc-lint` Graph Schema

This document records the current export contract for `sc-lint-boundary`.

## Versioning

- tool: `sc-lint-boundary`
- current schema version: `0.1.0`

Both graph JSON and findings JSON include:

- `tool`
- `version`
- `schema_version`

The schema version is the compatibility key for downstream graph consumers.

## Graph Export Formats

Supported export formats:

- `json`
- `turtle`

Current CLI:

```text
sc-lint-boundary export-graph --root <repo> --format json
sc-lint-boundary export-graph --root <repo> --format turtle
```

## Node Model

Current node kinds:

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

Current shared node fields:

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

Current edge kinds:

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

Current finding fields:

- `rule_id`
- `kind`
- `message`
- `owner_ids`
- `node_ids`

Current implemented rule ids:

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

Current implemented source-level allowances:

- `boundary.allow("cycle.type_method_self_loop")`
- `boundary.allow("cycle.recursive_value_container")`

`cycle.recursive_value_container` is intended for deliberate recursive
container/value models where every owner participating in the recursive cycle
opts in explicitly.

## Default Trait Policy

The analyzer includes an embedded default policy file:

- `crates/sc-lint-boundary/config/defaults.toml`

Current trait self-loop filtering is configured through:

- `trait_self_loop.ignored_trait_paths`
- `trait_self_loop.ignored_trait_names`

This keeps common ecosystem trait families configurable without baking
repo-specific names into analyzer code.

## Stability Notes

Stable enough to build against now:

- `tool`
- `schema_version`
- node/edge top-level field names
- current rule ids

Still expected to evolve:

- exact node kind inventory
- exact edge kind inventory
- which advisory findings are emitted by default
- Turtle vocabulary expansion
