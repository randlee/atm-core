# AQ3 tmux idle-transition-drain evidence

Host: `clean-runner-windows`
Commit: `35a28588ab4aa319eabbb972ca68931d9a61f05e`
Status: **SKIPPED** — tmux not available on Windows platform

## Reason

AQ3's tmux idle-transition drain is a Unix/Linux/macOS-only feature. Windows platforms do not provide tmux, so the evidence harness `run_aq3_tmux_idle_drain_evidence.py` self-reports `skipped_no_tmux` on Windows runners and is not executed.
