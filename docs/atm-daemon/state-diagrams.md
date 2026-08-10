# Daemon State Diagrams

These diagrams are the visual companion to
[startup-state-machine.md](./startup-state-machine.md).

Source of truth:
- authored Mermaid files live in `docs/atm-daemon/*.mmd`
- rendered report lives at [daemon-state-diagrams.html](../reports/daemon-state-diagrams.html)
- rendered panel fragments live under `docs/reports/panels/`

Generation:
- `python3 docs/reports/generate_diagram_pages.py`

Diagram rule:
- every state/transition must map to code-owned state, real events/guards, and
  caller-visible or `doctor`-visible outcomes that QA can test directly

Current implementation seam:
- historical/retired: `crates/atm-daemon/src/composition.rs` (deleted by
  AM.3)
- `crates/atm-daemon-client/src/lib.rs`

Diagrams:
- CLI bootstrap:
  [cli-bootstrap-state-machine.mmd](./cli-bootstrap-state-machine.mmd)
- daemon runtime lifecycle:
  [runtime-lifecycle-state-machine.mmd](./runtime-lifecycle-state-machine.mmd)
- reachable-daemon request outcomes:
  [reachable-daemon-request-outcome-state-machine.mmd](./reachable-daemon-request-outcome-state-machine.mmd)

Viewer:
- [daemon-state-diagrams.html](../reports/daemon-state-diagrams.html)
