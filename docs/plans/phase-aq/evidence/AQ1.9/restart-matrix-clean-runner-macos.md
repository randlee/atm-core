# AQ1.9 Hermes ATM restart matrix

Host: `clean-runner-macos`
Commit: `585eff5e4c5f55c762b0471cf36bf51319095fa5`

| Row | Status | Delivery latency | Evidence |
| --- | --- | ---: | --- |
| daemon-restart-live-receiver | PASS | 167.701 ms | `restart-matrix-clean-runner-macos.json` |
| receiver-restart-live-daemon | PASS | 34.803 ms | `restart-matrix-clean-runner-macos.json` |
| receiver-crash-within-window | PASS | 164.013 ms | `restart-matrix-clean-runner-macos.json` |

The daemon restart row keeps the receiver worker alive. The receiver restart row performs a clean close and immediate replacement. The crash row uses SIGKILL and starts the successor inside the active lease window; its one-refresh-tick assertion is recorded in JSON.

The m5 live run must be executed on m5; no remote result is inferred from this local artifact.
