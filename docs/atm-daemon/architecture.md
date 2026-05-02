# ATM-Daemon Crate Architecture

## 1. Purpose

This document defines the `atm-daemon` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns runtime composition only.

This crate is introduced by the Phase Q implementation line and is not part of
the current pre-Phase-Q workspace yet.

## 2. Responsibilities

The `atm-daemon` crate is responsible for:

- singleton daemon startup and ownership checks
- local daemon API listener
- remote daemon-to-daemon transport listener/client
- runtime wiring of `atm-core` service boundaries
- live agent-status cache
- optional watch/reconcile runtime loop
- daemon/runtime observability emission
- daemon health/status query surface for `atm doctor`

The `atm-daemon` crate must remain thin.

## 3. Architectural Rules

- `atm-daemon` must not reimplement `atm-core` business logic.
- `atm-daemon` must not access SQLite except through the `atm-core` store
  boundary.
- `atm-daemon` must not parse or write inbox JSONL except through the
  `atm-core` ingress/export boundaries.
- `atm-daemon` owns one protocol with two transport adapters:
  - Unix domain socket
  - TCP/TLS
- cross-host delivery is daemon-to-daemon only.
- remote delivery may use bounded transient retry, but not a durable long-lived
  remote outbox.
- remote send success is defined by remote daemon acceptance within the bounded
  retry window.
- daemon runtime failures must remain typed and must not depend on
  panic/unwrap for routine transport, socket, or store-boundary failure.
- daemon observability remains structured through `sc-observability`; no ad hoc
  debug-only runtime path replaces it in production.
- plugin-local observability does not replace daemon-owned runtime/transport
  sinks; daemon-owned events stay daemon-owned.

## 3.1 Singleton Runtime

Hard invariant:
- it must be impossible for two active ATM daemons to run on one host at the
  same time

Architectural rule:
- singleton enforcement belongs in the runtime wrapper only
- the runtime must fail closed rather than allowing split ownership

## 3.2 Status Ownership

The daemon owns the live runtime view of agent status.

Architectural rules:
- live status remains in daemon memory
- SQLite may retain a diagnostic snapshot only
- status cache rebuild after restart begins from `unknown` and refreshes through
  runtime events

## 3.3 Test Strategy

The daemon is not the core test strategy.

Architectural rules:
- `atm-daemon` should be testable primarily through in-process harnesses and
  fakes around its adapters
- if process-level daemon smoke tests exist, they must remain small and
  separate
- no core ATM correctness rule should require a real daemon process for normal
  validation
- `atm doctor` and other daemon-querying CLI flows must rely on explicit daemon
  request/response paths, not private inspection shortcuts

## 4. ADR Namespace

The `atm-daemon` crate uses the `ADR-DAEMON-*` namespace.

Initial use cases:

- singleton runtime enforcement
- local transport adapter structure
- remote daemon-to-daemon protocol structure
- runtime watch/reconcile orchestration
