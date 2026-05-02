# ATM Crate Architecture

## 1. Purpose

This document defines the `atm` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns only CLI-layer decisions.

## 2. Responsibilities

The `atm` crate is responsible for:

- clap argument parsing
- command dispatch into `atm-core`
- output selection and rendering
- process exit status mapping
- constructing and injecting the concrete observability adapter
- constructing the production daemon client / runtime request adapter
- maintaining the retained CLI subcommand surface, including `teams` and
  `members`

The `atm` crate must remain thin.

## 3. Architectural Rules

- `atm` may validate CLI syntax, but not reimplement `atm-core` business rules.
- `atm` may shape output, but not change core service semantics.
- `atm` owns mapping of CLI flags to `atm-core` request structs.
- `atm` owns mapping of CLI commands to the daemon/service request boundary in
  production.
- `atm` owns bootstrap of shared observability implementations used by
  `atm-core`.
- `atm` owns the concrete published-crate bootstrap against
  `sc-observability = "1.0.0"`.
- `atm` owns the structured construction contract for the concrete adapter:
  `CliObservability::new(home_dir, CliObservabilityOptions)`.
- `atm` may retain `init(...)` only as a delegating helper.
- `atm` owns CLI-layer observability for command entry, daemon connectivity,
  and render/exit outcomes.
- `atm` owns the retained local recovery CLI shape for `teams` and `members`,
  but not the underlying team/backup/restore business rules
- `atm` must not access SQLite or inbox JSONL directly
- `atm` must not own socket protocol semantics beyond client-side request
  mapping and error presentation
- `atm` must not auto-spawn the daemon in production
- `atm` must preserve typed runtime error identity until the rendering
  boundary instead of collapsing failures into panic/unwrap control flow

## 3.1 Phase Q CLI / Runtime Split

Phase Q keeps the CLI thin by enforcing this split:

- `atm` owns parse -> request mapping -> render
- `atm-core` owns business logic and service semantics
- `atm-daemon` owns runtime transport and singleton behavior

Test strategy rule:
- CLI tests must be able to target an in-process harness without requiring a
  daemon process

Doctor/runtime rule:
- `atm doctor` remains a CLI command, but its runtime-facing checks must query
  daemon state through the same daemon/service boundaries used by production

## 4. ADR Namespace

The `atm` crate uses the `ADR-ATM-*` namespace.

Initial use cases:

- clap surface decisions
- output-format decisions
- observability bootstrap wiring
- command-dispatch structure
