# AQ1.9 Hermes ATM restart matrix

Host: `clean-runner-windows`
Commit: `6644a533951f48d8d9def37775a38fac0df9aa90`

| Row | Status | Delivery latency | Evidence |
| --- | --- | ---: | --- |
| daemon-restart-live-receiver | PASS | 70.447 ms | `restart-matrix-clean-runner-windows.json` |
| receiver-restart-live-daemon | PASS | 50.704 ms | `restart-matrix-clean-runner-windows.json` |
| receiver-crash-within-window | PASS | 26.094 ms | `restart-matrix-clean-runner-windows.json` |

The daemon restart row keeps the receiver worker alive. The receiver restart row performs a clean close and immediate replacement. The crash row uses SIGKILL and starts the successor inside the active lease window; it passes only when `atm doctor --json` shows exactly one lease for the receiver at a new endpoint whose `registered_at` is at or before the successor's own `ready` event (`displaced_at_bind`: displaced by bind-time registration, zero refresh ticks) and the successor delivers the marker. `crash_recovery_ms`, `successor_spawn_to_ready_ms`, and `lease_displaced_at_ms` are recorded as diagnostics only, never asserted.

A `returncode` of 1 in `receiver_stop` / `daemon_cleanup` on Windows is `TerminateProcess` semantics for a terminated or killed child (harmless; recorded for provenance only).

The m5 live run must be executed on m5; no remote result is inferred from this local artifact.
