---
id: AI.18
title: atm-graft Python Bindings
status: complete
branch: feature/pAI-s18-graft-python-bindings
worktree: ../atm-core-worktrees/feature/pAI-s18-graft-python-bindings
target: integrate/phase-AI
---

# Sprint AI.18 — atm-graft Python Bindings

## Goal

Expose the full supported `atm-graft` host interface to Python through PyO3
and Maturin. The binding is a translation layer: it invokes the same sealed
`DaemonApiClient` boundary, preserves canonical message/address types, and
forwards the existing graft receiver callback without creating a Python
transport or host-specific message path.

## Hard Dependencies

- AI.17 is `PASS`.
- Phase AI’s HTTP client contract is available on the execution baseline.
- ADR-039 Python graft host binding is present and accepted on the execution
  baseline.

## Parallel Execution

AI.18 may run in parallel with AI.11–AI.16 after AI.17 `PASS`, provided the
sealed `DaemonApiClient` signature it consumes is unchanged. It adds a new
binding crate and must not modify `atm-graft`; a shared-contract change requires
rebase and renewed review.

## Deliverables

- A versioned Python package built with PyO3/Maturin, without changing the
  public Rust `atm-graft` API.
- Python coverage of the complete supported `atm-graft` host surface: client
  operations, receiver activation/lifecycle/snapshot/close, and canonical
  `HostNudgeInjector` callback registration. The callback forwards one
  canonical `PyNudge` through the existing receiver loop per delivered graft
  nudge.
- Typed Python representations for a canonical nudge and address, including
  optional `chat_id`; no string-only substitute for `AgentAddress`.
- A session is created with one typed caller address. `send(to, body)` uses
  that caller unchanged; `PyAgentAddress.chat_id` is the sole chat-id field.
- `PyNudge.source` exposes the canonical typed source address, including its
  optional `chat_id`; no derived representation exists.
- Tests for Python-to-Rust address round trip, session activation/snapshot/
  close lifecycle, nudge callback delivery, callback error propagation,
  absent/present chat ID, and no direct daemon socket or storage access from
  the binding.

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

#[pyclass]
struct PyGraftSessionSnapshot {
    agent: String,
    team: String,
    state: String,
}

#[pyclass]
struct PyGraftSessionOptions { workspace_root: String, agent: String, team: String }

#[pymethods]
impl PyGraftSession {
    #[new]
    fn new(caller: PyAgentAddress) -> PyResult<Self>;
    fn send(&self, to: PyAgentAddress, body: String) -> PyResult<()>;
    fn read(&self) -> PyResult<Vec<PyMessage>>;
    fn acknowledge(&self, message_id: String, reply_body: String) -> PyResult<()>;
    fn activate_receiver(
        &self,
        options: PyGraftSessionOptions,
        on_nudge: Py<PyAny>,
    ) -> PyResult<()>;
    fn snapshot(&self) -> PyResult<PyGraftSessionSnapshot>;
    fn close(&self) -> PyResult<()>;
}

```

Python receives typed address fields; it does not receive a raw daemon request
or construct a transport envelope. The `caller` supplied to `new` is the
single source of caller `chat_id`; `to` is the destination address. A callback
registered by `activate_receiver` is the Python representation of the existing
`HostNudgeInjector`: it receives `PyNudge` from the existing graft callback and
cannot alter persistence, routing, acknowledgement, or retry behavior.

## Boundary and Non-Goals

The binding does not add a second Python transport, custom write request,
schema migration, polling loop, callback retry queue, or message persistence.
It may not add a `session_id` field or a `--session` compatibility surface.
It may wrap public `atm-graft` host types, but it may not change the public
Rust `atm-graft` API.

## Closure

- Focused Rust/Python tests, `just lint`, `just test`, and `git diff --check`
  pass.
- The completion inventory names each exported Python type/method and its
  underlying graft symbol, including receiver callback and session lifecycle
  coverage.
