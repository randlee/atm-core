# AQ1.9 Hermes ATM restart matrix

Host: `clean-runner-linux`
Commit: `a2dc79e52830e392f9f86bcc6aff46f549a4956e`

| Row | Status | Delivery latency | Evidence |
| --- | --- | ---: | --- |
| daemon-restart-live-receiver | PASS | 21.217 ms | `restart-matrix-clean-runner-linux.json` |
| receiver-restart-live-daemon | PASS | 25.226 ms | `restart-matrix-clean-runner-linux.json` |
| receiver-crash-within-window | PASS | 25.255 ms | `restart-matrix-clean-runner-linux.json` |

The daemon restart row keeps the receiver worker alive. The receiver restart row performs a clean close and immediate replacement. The crash row uses SIGKILL and starts the successor inside the active lease window; its one-refresh-tick assertion is recorded in JSON.

The m5 live run must be executed on m5; no remote result is inferred from this local artifact.
