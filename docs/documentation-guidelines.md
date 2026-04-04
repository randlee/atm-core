# ATM-Core Documentation Guidelines

## 1. Purpose

This document defines how `atm-core` product and crate documentation is
organized.

The goals are:

- one clear source of truth for each requirement or architectural decision
- explicit ownership boundaries between product documentation and crate
  documentation
- no duplicated requirement text across files
- traceability from product behavior to crate implementation responsibility
- easier review of boundary leakage between `atm` and `atm-core`

## 2. Principles

### 2.1 One Requirement, One Home

A requirement must be written in exactly one place.

- Product-level behavior belongs in product-level requirements.
- Crate-level implementation obligations belong in crate-level requirements.
- A document may reference a requirement owned elsewhere, but it must not copy
  the requirement text.

### 2.2 Product Docs Define Behavior

Files in `docs/` at the top level define:

- the product contract
- the system architecture
- the implementation plan
- cross-cutting behavior that spans more than one crate

Top-level docs must not drift into crate-local implementation detail unless that
detail is necessary to explain a product-level decision.

### 2.3 Crate Docs Define Ownership

Files in `docs/atm/` and `docs/atm-core/` define:

- what each crate owns
- how each crate satisfies referenced product requirements
- crate-local API and module boundaries
- crate-local architectural decisions

Crate docs must not redefine the product contract.

### 2.4 Traceability Over Duplication

When a product requirement is implemented by one or more crates:

- the product requirement references the owning crate requirement IDs
- the crate requirement references the product requirement it satisfies

This traceability is required so reviewers can see:

- missing ownership
- overlapping ownership
- boundary leakage

### 2.5 Boundary Leaks Must Be Obvious

The documentation structure should make these failures easy to detect:

- `atm` owning core workflow logic
- `atm-core` owning clap/terminal/UI behavior
- product behavior duplicated in multiple files
- two crates both claiming the same responsibility

Concrete example:

- if `docs/atm-core/` starts defining clap flag semantics such as the exact
  meaning of `atm read --history`, that is a boundary leak; flag parsing and
  command-surface ownership belong in `docs/atm/`, while `docs/atm-core/`
  should own only the underlying selection/state behavior

## 3. Directory Layout

The required documentation layout is:

```text
docs/
  documentation-guidelines.md
  requirements.md
  architecture.md
  claude-code-message-schema.md
  atm-message-schema.md
  legacy-atm-message-schema.md
  sc-observability-schema.md
  project-plan.md
  read-behavior.md
  archive/
    file-migration-plan.md
    migration-map.md
    obs-gap-analysis.md
  atm/
    requirements.md
    architecture.md
    commands/
      send.md
      read.md
      ack.md
      clear.md
      log.md
      doctor.md
  atm-core/
    requirements.md
    architecture.md
    modules/
      send.md
      read.md
      ack.md
      clear.md
      log.md
      doctor.md
      mailbox.md
      config.md
      observability.md
```

Notes:

- Additional supporting docs may be added under `docs/atm/` or
  `docs/atm-core/` when justified.
- Top-level docs remain the only product-level source of truth.
- Cross-subsystem schema ownership docs that define who owns a wire/storage
  schema belong at top level and must use explicit subsystem names in the file
  name.
- Command docs belong under `docs/atm/commands/`.
- Core service and module ownership docs belong under `docs/atm-core/modules/`.

Schema ownership file naming rules:

- Claude Code-native schema docs must include `claude`, `code`, and `schema`
  in the filename.
- ATM additive/interpreted schema docs must include `atm` and `schema` in the
  filename.
- Legacy compatibility schema docs should include both the owning subsystem
  name and `schema` in the filename so read-only compatibility contracts are
  explicit rather than implied.
- Shared subsystem schema pointers, such as `sc-observability`, should be
  co-located with the ATM and Claude Code schema docs and should point to the
  owning external repository instead of redefining that subsystem locally.

Schema enforcement rules:

- Every schema defined locally in `docs/` must have a corresponding enforcement
  model in source control.
- Python/Pydantic enforcement models for top-level schema docs live under
  `tools/schema_models/`.
- External schema pointer docs, such as `sc-observability`, may omit a local
  Pydantic model when this repository does not own the schema definition.
- Source files that parse or serialize a locally documented schema must include
  comments pointing to the owning schema doc and must not silently redefine an
  externally owned schema.

## 4. Top-Level Document Responsibilities

### 4.1 `docs/requirements.md`

Owns:

- retained product surface
- user-visible behavior
- command semantics
- mailbox/workflow behavior
- external contracts
- product-level non-functional requirements

Must not own:

- crate-local clap wiring
- crate-local module layouts
- crate-local implementation details beyond what is needed to define behavior

### 4.2 `docs/architecture.md`

Owns:

- system shape
- crate boundaries
- shared models
- integration boundaries
- cross-cutting architecture decisions

Must not duplicate full crate-local API specs when those are owned by crate
docs.

### 4.3 `docs/project-plan.md`

Owns:

- work phases
- sequencing
- milestones
- acceptance gates
- migration strategy

Must reference requirement and architecture IDs instead of restating those
contracts in full.

### 4.4 Supporting Top-Level Docs

Top-level supporting docs are allowed only when they remain cross-cutting.

Examples:

- `read-behavior.md`

Migration-only supporting documents now live under `docs/archive/`:

- `archive/file-migration-plan.md`
- `archive/migration-map.md`
- `archive/obs-gap-analysis.md`

If a supporting document becomes crate-specific, move it under the owning crate
directory.

If a supporting document exists only for the migration program, mark it
explicitly as migration-phase/temporary and remove it once its role is complete.

## 5. Crate-Level Document Responsibilities

### 5.1 `docs/atm/`

Owns CLI-specific documentation:

- clap command surfaces
- flag semantics owned by the CLI layer
- human-readable and JSON output contracts
- command dispatch boundaries
- `atm` architectural decisions

`docs/atm/commands/` owns one file per retained command.

Each command file must document:

- the command entrypoint
- CLI-owned flags and parsing rules
- how the command maps into `atm-core`
- output shaping and rendering behavior owned by `atm`
- references to the product and core requirements it depends on

### 5.2 `docs/atm-core/`

Owns core library documentation:

- service/API ownership
- state machines
- typestate and transition rules
- mailbox/config/observability boundaries
- module-level contracts
- `atm-core` architectural decisions

`docs/atm-core/modules/` owns one file per significant module or service area.

Each module file must document:

- the module’s responsibility
- inputs and outputs
- invariant rules
- referenced product requirements
- referenced crate-level requirements and ADRs

## 6. Requirement IDs

Formal requirement IDs are required.

Use these prefixes:

- `REQ-P-*` for product-level requirements
- `REQ-ATM-*` for `atm` crate requirements
- `REQ-CORE-*` for `atm-core` crate requirements

ID rules:

- IDs must be stable once published
- IDs must not be reused for unrelated requirements
- requirement text must appear only at the owning ID location

Examples:

- `REQ-P-READ-001`
- `REQ-ATM-LOG-001`
- `REQ-CORE-MAILBOX-003`

## 7. Architecture Decision IDs

Formal ADR IDs are required.

Use these prefixes:

- `ADR-P-*` for product-level architecture decisions
- `ADR-ATM-*` for `atm` decisions
- `ADR-CORE-*` for `atm-core` decisions

Examples:

- `ADR-P-001`
- `ADR-ATM-002`
- `ADR-CORE-004`

## 8. Referencing Rules

### 8.1 Product To Crate

A product requirement must reference the crate requirement IDs that satisfy it.

### 8.2 Crate To Product

A crate requirement must reference the product requirement IDs it implements.

### 8.3 ADR References

When a requirement depends on an architectural decision, reference the ADR ID
instead of repeating the decision text.

### 8.4 File References

When a document references implementation files, use exact repo-relative paths.
Do not rely on ambiguous prose references.

## 9. Migration Rules For Existing Docs

The existing top-level docs are the starting point. They must be cleaned up
into this structure incrementally.

Required migration order:

1. create crate directories and crate-level skeleton docs
2. assign requirement and ADR ID namespaces
3. move crate-local detail out of top-level docs into owning crate docs
4. replace duplicated prose with references
5. keep top-level product docs concise and cross-cutting

Migration-phase supporting docs such as `read-behavior.md`,
`file-migration-plan.md`, and `migration-map.md` must be explicitly classified
as either:

- permanent cross-cutting documents
- or temporary migration artifacts

Do not leave their lifecycle implicit.

During migration:

- do not delete product-level requirements without rehoming them
- do not duplicate content as a temporary “copy first” step unless immediately
  followed by removal from the old location in the same change
- note unresolved ownership gaps explicitly instead of leaving them implicit

## 10. Review Checklist

Before a documentation change is review-ready, verify:

- every new requirement has exactly one owning file
- every product requirement has crate-level ownership references where needed
- no crate doc restates the full product requirement text
- file references use exact repo-relative paths rather than ambiguous prose
- command docs live under `docs/atm/commands/`
- core module docs live under `docs/atm-core/modules/`
- boundary ownership between `atm` and `atm-core` is explicit
- requirement and ADR IDs are stable and correctly prefixed
- the top-level docs stay readable as product documents rather than devolving
  into crate-internal notes
