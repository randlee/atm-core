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

The `atm` crate must remain thin.

## 3. Architectural Rules

- `atm` may validate CLI syntax, but not reimplement `atm-core` business rules.
- `atm` may shape output, but not change core service semantics.
- `atm` owns mapping of CLI flags to `atm-core` request structs.
- `atm` owns bootstrap of shared observability implementations used by
  `atm-core`.
- `atm` owns the temporary pre-publish dependency wiring for a local
  `sc-observability` checkout until the shared crates are published.

## 4. ADR Namespace

The `atm` crate uses the `ADR-ATM-*` namespace.

Initial use cases:

- clap surface decisions
- output-format decisions
- observability bootstrap wiring
- command-dispatch structure
