# ADR-ATM-RUNTIME-001 — `atm-runtime` As Concrete Composition Root

```yaml
adr_id: ADR-ATM-RUNTIME-001
crate: atm-runtime
title: atm-runtime as concrete composition root
status: proposed
date: 2026-05-30
deciders:
  - team-lead
  - arch-ctm
tags:
  - runtime
  - composition
  - boundaries
related_boundaries:
  - BOUNDARY-RuntimeFactory
  - BOUNDARY-AtmRuntime-Composition
code_references:
  - docs/phase-AA/sprint-AA2.md
  - docs/phase-AA/sprint-AA3.md
  - docs/atm-runtime/architecture.md
```

## Context

`atm-daemon` drifted into direct SQLite-aware composition, health probing,
observability wiring, and replay-store ownership. The daemon is intended to be
a thin router and must not know the concrete storage backend.

Phase AA introduces a new crate to own the concrete runtime/store composition
work that currently lives in the wrong place.

## Decision

Introduce `atm-runtime` as the concrete composition root.

`atm-runtime` owns:
- `SqliteBoundaryAssembly` construction
- concrete store/replay assembly
- concrete `ConfigDoctor` assembly
- `RuntimeBundle` assembly for CLI and daemon callers

`atm-runtime` does not own:
- daemon transport
- daemon lifecycle
- CLI rendering
- backend-specific SQLite logic that belongs inside `atm-rusqlite`

## Alternatives Considered

- keep concrete composition in `atm-daemon`
- move the composition burden directly into `atm`
- let `atm` and `atm-daemon` each assemble SQLite independently

## Consequences

- `atm-daemon` can become storage-agnostic again
- the direct CLI doctor path can depend on `atm-runtime` without taking a
  direct dependency on `atm-rusqlite`
- the new crate requires explicit boundary governance so its dependency edges
  do not become another silent leak point
