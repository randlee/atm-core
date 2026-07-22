---
id: AI.18
title: atm-graft Python Bindings
status: planned
branch: feature/pAI-s18-graft-python-bindings
worktree: ../atm-core-worktrees/feature/pAI-s18-graft-python-bindings
target: integrate/phase-AI
---

# Sprint AI.18 — atm-graft Python Bindings

## Goal

Expose the existing typed `atm-graft` interface to Python through PyO3 and
Maturin. The binding is a translation layer: it invokes the same sealed
`DaemonApiClient` boundary and preserves the canonical message/address types.

## Hard Dependencies

- AI.17 is `PASS`.
- Phase AI’s HTTP client contract is available on the execution baseline.

## Deliverables

- A versioned Python package built with PyO3/Maturin, without changing the
  public Rust `atm-graft` API.
- `AtmGraftSession` lifecycle methods that wrap the existing graft session.
- Typed Python representations for a canonical nudge and address, including
  optional `chat_id`; no string-only substitute for `AgentAddress`.
- `send(to_agent, message, team=None, chat_id=None)` maps the optional
  **caller** `chat_id` to the existing caller address; callers may alternatively supply the equivalent
  qualified caller form supported by Phase AI.
- Tests for Python-to-Rust address round trip, nudge callback delivery, error
  propagation, absent/present chat ID, and no direct daemon socket or storage
  access from the binding.

## Exact Targets and API

- `crates/atm-graft-python/` — new PyO3 crate; depends only on `atm-graft`
  and shared typed model crates, never daemon/runtime/storage crates.
- `crates/atm-graft-python/src/lib.rs` — Python module implementation.
- `crates/atm-graft-python/tests/python_api.rs` and a checked-in Maturin
  smoke script — binding behavior and package-import proof.

```rust
#[pyclass]
struct PyAgentAddress { agent: String, chat_id: Option<String>, team: String }

#[pyclass]
struct PyNudge { message_id: String, source: PyAgentAddress, body: String }

#[pymethods]
impl PyGraftSession {
    fn send(&self, to: PyAgentAddress, body: String, chat_id: Option<String>) -> PyResult<()>;
    fn read(&self) -> PyResult<Vec<PyMessage>>;
    fn acknowledge(&self, message_id: String) -> PyResult<()>;
}
```

Python receives typed address fields; it does not receive a raw daemon request
or construct a transport envelope. A callback registration method may be added
only if it forwards `PyNudge` from the existing graft callback.

## Boundary and Non-Goals

The binding does not add a second Python transport, custom write request,
schema migration, polling loop, or message persistence. It may not add a
`session_id` field or a `--session` compatibility surface.

## Closure

- Focused Rust/Python tests, `just lint`, `just test`, and `git diff --check`
  pass.
- The completion inventory names each exported Python type/method and its
  underlying graft symbol.
