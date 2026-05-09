# ATM Crate Architecture

## 1. Purpose

This document defines the `atm` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns only CLI-layer decisions.

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

The canonical daemon packet contract lives in:
- [`../atm-daemon/protocol-icd.md`](../atm-daemon/protocol-icd.md)

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

Phase R redesign notes:
- the CLI should depend on `AtmProtocol` and `ClientTransport`, not on daemon
  internals or SQLite adapters
- retained user-facing workflows may still include `ack`, but thin-client
  transport shape should stay centered on `send` and `receive`
- the CLI same-host client transport should use the same ATM frame helper layer
  as daemon local IPC and remote peer transport rather than a CLI-only framing
  path
- the current daemon packet family serves `send`, `ack`, `read`, `clear`, and
  `doctor`; retained `log`, `teams`, and `members` stay outside the daemon
  request/response packet surface in the current Phase S line

## 1.1 ADRs

## CLI uses shared protocol and client transport only

```yaml
adr_id: ADR-ATM-001
crate: atm
title: CLI uses shared protocol and client transport only
status: accepted
date: 2026-05-03
deciders:
  - team-lead
  - arch-ctm
tags:
  - protocol
  - transport
  - privacy
related_boundaries:
  - BOUNDARY-AtmProtocol
  - BOUNDARY-ClientTransport
  - BOUNDARY-ClientTransport-CLI
code_references:
  - docs/atm/boundaries.md
  - docs/atm-core/boundaries.md
```

Context:
- Phase Q drift showed that letting CLI reach daemon internals or SQLite
  adapters made architecture violations easy and review expensive.

Decision:
- The CLI depends on `AtmProtocol` and `ClientTransport` only.
- It must not depend on daemon internals or SQLite adapter crates.
- Retained user workflows may still include `ack`, but thin-client transport
  shape remains `send` / `receive`.

Consequences:
- CLI runtime wiring stays thin.
- Thin extension crates can mirror the same client shape without importing CLI
  internals.

Alternatives considered:
- Let CLI call daemon internals directly.
- Let CLI use concrete SQLite adapters for local shortcuts.

Follow-up work:
- Enforce the forbidden dependency edges in lint.
- Keep CLI help and request mapping aligned with the thin-client shape.

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
- `atm` must own the one documented daemon auto-start path in production and
  must not silently bypass the daemon if startup fails
- daemon auto-start must be an explicit runtime-entry step, not a hidden side
  effect of transport object construction
- the client-side launch path must acquire the documented pre-spawn launch gate
  before daemon fork/exec
- `atm` must preserve typed runtime error identity until the rendering
  boundary instead of collapsing failures into panic/unwrap control flow

## 3.1 Phase R CLI / Runtime Split

Phase R keeps the CLI thin by enforcing this split:

- `atm` owns parse -> request mapping -> render
- `atm-core` owns business logic and service semantics
- `atm-daemon` owns runtime transport and singleton behavior

Test strategy rule:
- CLI tests must be able to target an in-process harness without requiring a
  daemon process
- `CliComposition::from_transport(...)` is the primary seam for fake or
  loopback transport tests

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
