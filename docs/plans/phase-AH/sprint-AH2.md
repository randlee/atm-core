---
id: AH.2
title: atm-graft PyO3 Python Bindings
status: planned
branch: feature/pAH-s2-pyo3-bindings
worktree: ../atm-core-worktrees/feature/pAH-s2-pyo3-bindings
target: develop
---

# Sprint AH.2 — atm-graft PyO3 Python Bindings

```yaml
plan_type: sprint_plan
phase: AH
sprint: AH.2
worktree: ../atm-core-worktrees/feature/pAH-s2-pyo3-bindings
branch: feature/pAH-s2-pyo3-bindings
status: planned
estimated_scope: medium
```

## Goal

Produce a Python extension module that wraps atm-graft for Python host
agents. The binding exposes:

- `AtmGraftSession` — a Python class wrapping `atm-graft::GraftSession`
- `set_nudge_callback()` — accepts a Python callable implementing the
  equivalent of `HostNudgeInjector::inject_nudge()`
- transparent auto-population of `session_id` on outbound sends from
  `HERMES_SESSION_KEY` env

This sprint does NOT touch Hermes. It only proves that atm-graft can be
consumed from Python with the required callback seam.

## Hard Dependencies

- Phase AF accepted release baseline (`develop` at `98a4e66c` or later)
- atm-graft 1.3.1 Rust API is stable; this sprint may not change atm-graft
  source
- AH.1 is `PASS` — the Python binding must carry `session_id` through the
  Python→Rust boundary verbatim
- PyO3 0.20+ and Maturin 1.4+ tooling

## Exact Targets

- `crates/atm-graft-python/Cargo.toml` — new crate (cdylib type)
- `crates/atm-graft-python/pyproject.toml` — Maturin build metadata
- `crates/atm-graft-python/src/lib.rs` — Python extension module
- `crates/atm-graft-python/src/session.rs` — `AtmGraftSession` bindings
- `crates/atm-graft-python/src/nudge_callback.rs` — Python→Rust callback
  adapter
- workspace `Cargo.toml` update to include the new crate
- Python unit tests under `crates/atm-graft-python/tests/`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims.

- new crate `crates/atm-graft-python/` with a clean module boundary between
  session lifecycle and nudge-callback adapter
- Maturin build produces a loadable Python extension on macOS (Apple
  Silicon)
- `AtmGraftSession` class with:
  - `__init__(team, agent, workspace_root)`
  - `activate(callback)` — starts the graft session and the nudge-receiver
    loop
  - `deactivate()` — shuts down cleanly
  - `send(to_agent, message, team=None, session_id=None)` — wraps the
    atm-graft send; `session_id` defaults to `HERMES_SESSION_KEY` when unset
  - `read(scope=None)` — wraps `atm read` query modes; defaults to
    session-scoped via `HERMES_SESSION_KEY`
- Python nudge callback adapter — when the Rust nudge receiver loop
  receives a nudge, the Python callable is invoked on the Python thread with
  an `AtmNudge` object containing `sender`, `team`, `body`, `session_id`
- error propagation across FFI boundary: atm-graft Rust `Result` maps to
  Python exceptions with typed messages
- docs under `crates/atm-graft-python/README.md` plus inline docstrings

## Required Work

### Maturin integration

- `Cargo.toml` declares `crate-type = ["cdylib"]`
- pyo3 features gated on `pyo3/extension-module`
- `pyproject.toml` declares the Maturin build backend; `python_requires`
  at `>=3.11`
- workspace `Cargo.toml` adds `crates/atm-graft-python` to members
- existing atm-graft Rust API must not be modified or duplicated; the new
  crate depends on it via workspace path

### Python API surface

```python
import atm_graft_python as atm_graft

def on_nudge(nudge: atm_graft.AtmNudge) -> None:
    print(f"from: {nudge.sender}@{nudge.team}")
    print(f"session: {nudge.session_id}")
    print(f"body: {nudge.body}")

session = atm_graft.AtmGraftSession(
    team="hermes",
    agent="hendrix",
    workspace_root="/Users/randlee/Documents/github/hendrix"
)
session.activate(on_nudge)

# auto-attach session_id from HERMES_SESSION_KEY
session.send(to_agent="arch-ctm", message="design question?")

# explicit session_id
session.send(to_agent="arch-ctm", message="follow-up", session_id="uuid-xyz")

# session-scoped read (uses HERMES_SESSION_KEY)
for msg in session.read():
    print(msg)

# peer-scoped read
for msg in session.read(scope=atm_graft.Scope.agent("arch-ctm")):
    print(msg)

session.deactivate()
```

### Error propagation

- Rust `AtmError` maps to Python `AtmGraftError`
- daemon-unreachable errors, auth errors, protocol errors all surface
  distinctly
- Python callback exceptions are caught inside the Rust loop and surfaced
  via the atm-graft `GraftObservability::session_error()` port (no Rust
  panic across the FFI boundary)

### Tests

- Python module imports cleanly
- `AtmGraftSession` activates and deactivates with a mock daemon
- nudge callback fires with synthetic nudge event from a mock loop
- error propagation: daemon unreachable raises typed exception
- `session_id` round-trip on send: explicit value preserved through the
  Python→Rust boundary; `None` lets the Rust side default from
  `HERMES_SESSION_KEY`
- scope-aware `read()`: session-scoped, peer-scoped, explicit-id-scoped
  all hit correct CLI paths

## Non-Closure

This sprint does not:

- integrate with Hermes (AH.3)
- add a launchd bridge process (AH.4)
- validate any end-to-end story (AH.5)

## Acceptance Criteria

- the Python module builds and loads on macOS (Apple Silicon) Python 3.11+
- `AtmGraftSession.activate(callback)` runs without raising on a live
  atm-daemon 1.3.1
- `AtmGraftSession.send(...)` with no session_id auto-attaches from
  `HERMES_SESSION_KEY` env
- the Python nudge callback fires within <500ms of the daemon delivering
  the nudge to the atm-graft socket
- no atm-graft Rust source is modified by this sprint

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `maturin build --release` produces a wheel loadable by `pip install`
- `python -c "import atm_graft_python; atm_graft_python.AtmGraftSession"`
  succeeds against a live atm-daemon
- `git diff --check`
