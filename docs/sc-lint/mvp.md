# `sc-lint-boundary` MVP

## Purpose

`sc-lint-boundary` is the planned internal Rust analyzer for AST-sensitive
boundary linting.

The immediate driver is the current `cargo-modules --acyclic` noise in
`just lint modules`:

- self type <-> method loops
- newtype/conversion helper loops
- other item-level cycles that are not architectural boundary violations

The goal of the MVP is not to replace the current Python lint suite.
The goal is to add one precise Rust-native analyzer where Python and
third-party graph tools are too weak or too noisy.

## Product Position

The intended long-term shape is:

- internal workspace crate first
- separate repository later
- crates.io publication later

The crate should be designed from day one as a generic tool, not a
consumer-specific tool.

## Name

- package: `sc-lint-boundary`
- library crate: `sc_lint_boundary`
- binary: `sc-lint-boundary`
- initial crate version: `0.1.0`

Implemented paired attribute crate:

- package: `sc-lint-attributes`
- library crate: `sc_lint_attributes`
- initial crate version: `0.1.0`

These crates should **not** inherit the host workspace package version. They are
new tool crates with their own version line starting at `0.1.0`.

## Non-Goals

The MVP does **not** attempt to:

- replace the existing Python orchestration layer
- replace simple manifest/text/config lints
- perform full Rust name/type resolution
- expand macros like rustc
- implement every future boundary rule from the survey document
- integrate directly with a graph database

## Core Design

The analyzer should be designed as a **graph builder plus rule engine**.

Cycle detection is only the first rule family. The primary internal product is
an in-memory code graph that can later support:

- cycle analysis
- visibility/sealed checks
- unsafe ownership checks
- type-coupling analysis
- future graph export into external tooling

## Ownership Split

### Python remains responsible for

- `just` command orchestration
- repo-local TOML config loading
- wrapping third-party tools
- logging
- artifact generation / site generation
- simple manifest and text checks

### `sc-lint-boundary` becomes responsible for

- AST parsing with `syn`
- graph construction
- AST-sensitive boundary rules
- machine-readable findings for semantic Rust-code checks

### `sc-lint-attributes` becomes responsible for

- compile-valid attribute definitions
- a stable attribute namespace reserved for source-level declarations
- staying intentionally small and declarative

## Input Model

The analyzer should accept:

- repo root
- optional config path
- optional output path
- optional rule selection

Suggested CLI shape:

```text
sc-lint-boundary analyze --root <repo> --format json
sc-lint-boundary analyze --root <repo> --rule cycles --format text
sc-lint-boundary export-graph --root <repo> --output graph.json
```

## Discovery Model

Use:

- `cargo_metadata` for workspace/package/target discovery
- `walkdir` for file traversal
- `syn` for parsing source files

The analyzer should discover crate roots from Cargo metadata, not from
hard-coded repo layout assumptions.

## Internal Graph

The analyzer should build a property graph in memory with stable ids.

### MVP node kinds

- `crate`
- `module`
- `type`
- `variant`
- `field`
- `trait`
- `function`
- `method`
- `impl`

### MVP edge kinds

- `contains`
- `declares`
- `implements`
- `references`
- `calls`
- `uses_type`
- `belongs_to_module`
- `belongs_to_crate`

This should be enough for the first cycle rules while still leaving room for
future expansion.

## Rule Model

### MVP rule family

Only one rule family is required initially:

- cycle analysis

### MVP cycle goal

Distinguish:

- tool-noise / self-loop shapes
- actual architectural cycles spanning multiple owners

### MVP pass/fail rule

Fail only on cycles that involve:

- `2+` distinct owners

Where "owner" should be one of:

- module
- type
- trait

The exact owner level can be tuned during implementation, but the rule must not
fail on trivial self loops such as:

- `Type <-> Type::method`
- transparent newtype conversion helper loops

### MVP findings

Emit findings with stable rule ids and categories, for example:

- `SCB-CYCLE-001`
- `kind = "architectural_cycle"`
- `kind = "self_item_loop"`
- `kind = "newtype_conversion_loop"`

The finding shape should already be stable enough to survive later extraction.

## Output Model

The analyzer should support:

- text summary for local use
- JSON findings for the Python runner
- graph export

The current trait-self-loop default policy should be data-driven rather than
hardcoded in analyzer logic. The initial implementation uses an embedded
default TOML config with:

- exact ignored trait paths
- ignored terminal trait names for imported/common traits

The analyzer should also support explicit source-level allowances for accepted
recursive container/value models via:

- `#[sc_lint(boundary.allow("cycle.recursive_value_container"))]`

Current scaffold status:

- `sc-lint-boundary` exists and can already:
  - discover workspace crates through `cargo_metadata`
  - traverse inline and file-backed modules through `syn`
  - emit graph nodes for:
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
  - emit a stable text findings summary
  - emit JSON findings
  - export graph in:
    - JSON
    - Turtle
  - classify owner-graph cycles into:
    - `SCB-CYCLE-001` multi-owner architectural cycle
    - `SCB-CYCLE-002` type/method self-loop
    - `SCB-CYCLE-003` trait-impl self-loop
  - enforce:
    - `SCB-BOUNDARY-001` internal_only visibility violation
    - `SCB-BOUNDARY-002` internal_only external reference
    - `SCB-BOUNDARY-003` forbid_external_impls violation
- `sc-lint-attributes` exists and already reserves the shared
  `#[sc_lint(...)]` namespace with compile-valid, no-op attribute parsing

Current implemented attribute ingestion in the analyzer:

- `#[sc_lint(boundary.allow("..."))]`
- `#[sc_lint(boundary.internal_only)]`

Current implemented suppression use:

- `boundary.allow("cycle.type_method_self_loop")`
  - suppresses `SCB-CYCLE-002` on annotated items

Current rule behavior:

- `SCB-CYCLE-001`
  - hard failure
  - owner SCC with `2+` distinct owners
- `SCB-CYCLE-002`
  - non-fatal signal
  - inherent type/method self-loop
- `SCB-CYCLE-003`
  - non-fatal signal
  - trait-impl self-loop downgraded out of the main failure path

### JSON findings envelope

Suggested high-level shape:

```json
{
  "tool": "sc-lint-boundary",
  "version": "0.1.0",
  "status": "pass",
  "findings": []
}
```

### Graph export

The graph export should be a first-class design target, even if it is not the
main deliverable in the first implementation pass.

The recommended canonical graph export is generic JSON:

```json
{
  "nodes": [],
  "edges": []
}
```

Current supported exports:

- JSON
- Turtle

Current compatibility marker:

- `schema_version = "0.1.0"`

Do **not** target a specific graph database format in the MVP.

That export should later be usable for:

- graph DB import
- custom visualization
- offline analysis
- future lint rules

## Attributes

### Design direction

The analyzer should be designed with source-level attributes in mind.

The preferred use of attributes is **declarative boundary intent**, not
last-minute suppression.

Good future uses:

- internal-only items
- forbidden external implementation
- forbidden external use classes
- boundary roots / ownership declarations

### Important constraint

Real Rust attributes such as:

```rust
#[sc_lint(boundary.internal_only)]
```

require a separate proc-macro crate.

This design now assumes that crate exists from the start as
`sc-lint-attributes`, even if the first implementation is only a placeholder.

The initial supported syntax is:

```rust
#[sc_lint(boundary.allow("cycle.type_method_self_loop"))]
#[sc_lint(boundary.internal_only)]
```

### MVP attribute stance

The MVP should:

- reserve the attribute namespace in documentation and rule design
- avoid source-comment suppression for semantic cycle checks
- create the `sc-lint-attributes` crate early, even if it begins with minimal
  no-op attribute support

Once attributes are introduced, they should be:

- rule-specific
- declarative
- auditable

and **not** broad “ignore everything here” escapes.

### Preferred early use of attributes

Attributes are primarily for declarative source-level boundary intent, not
suppression. Strong early candidates include:

- internal-only items
- forbidden external implementation
- forbidden external use classes
- boundary roots / ownership declarations

Suppression attributes, if ever added, should come later and remain
rule-specific.

## Why Not Expand Python Further

We intentionally do **not** want complicated Rust parsing logic in Python.

Reasons:

- AST-sensitive logic quickly becomes brittle in Python
- semantic cycle classification is not well expressed by string matching
- repo-specific symbol allowlists are the wrong direction

This analyzer exists to prevent that drift.

## Relationship to Current Tools

### Keep

- Python lint orchestration
- `cargo-deny`
- `cargo-shear`
- `codespell`
- manifest/version checks
- boundary-doc/schema checks
- report/site generation

### De-emphasize

- `cargo-modules --acyclic` as the source of truth for architectural cycle
  failures

`cargo-modules` may still remain useful for:

- visualization
- advisory structure checks
- dead-file/orphan detection

but the analyzer should own semantic cycle enforcement.

## MVP Deliverables

1. New internal workspace crate:
   - `crates/sc-lint-boundary`
2. New internal workspace crate:
   - `crates/sc-lint-attributes`
3. CLI binary:
   - `sc-lint-boundary`
4. Minimal compile-valid attribute namespace in `sc-lint-attributes`
5. Workspace/package/target discovery via `cargo_metadata`
6. AST parsing via `syn`
7. Internal graph IR for crate/module/type/method ownership
8. JSON graph export
9. JSON findings output
10. First rule family:
   - cycle classification
   - self-loop suppression
   - fail only on multi-owner cycles
11. Python integration point documented for future `just lint` wiring

## Explicit Deferrals

- full visibility/sealed enforcement
- unsafe policy enforcement
- graph database integration
- full dead-code detection
- cross-crate type-resolution accuracy beyond explicit syntax edges

## Recommended First Sprint Cut

### Step 1

Create `sc-lint-boundary` as an internal CLI crate with:

- CLI skeleton
- config skeleton
- `cargo_metadata` workspace discovery
- `syn` parsing for workspace targets

Create `sc-lint-attributes` alongside it with:

- proc-macro crate skeleton
- reserved attribute namespace
- minimal/no-op compile-valid attribute entry points

### Step 2

Build the internal graph for:

- crates
- modules
- types
- methods
- impls

### Step 3

Implement the first cycle rule family and classify:

- self-item loops
- type-method self loops
- obvious newtype conversion loops
- multi-owner cycles

### Step 4

Emit:

- human text summary
- JSON findings
- graph JSON export

## Success Criteria

The MVP is successful if:

- it eliminates the current `cargo-modules` false positives without repo-specific
  symbol allowlists
- it keeps Python out of AST-sensitive Rust parsing
- it produces a reusable graph export for future rule work
- it establishes the source-attribute crate early enough that later adoption
  does not require packaging surgery
