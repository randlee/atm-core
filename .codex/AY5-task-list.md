# AY5 — Transactional Herdr entry control plane

- [x] Define REQ-P-DAEMON-SWITCH-002 and the ADR-053 entry-management addendum (`2017b1291`).
- [x] Add native `atm doctor --json` ingestion and validated endpoint model (`080455a18`).
- [x] Implement deterministic identifiers, canonical platform definitions, marker/digest checks, and exactly-one-object JSON output (`080455a18`).
- [x] Implement durable install/remove journals and fail-closed status repair using platform fakes (`080455a18`).
- [x] Add macOS, Linux, and Windows fixture coverage for normal, foreign, mismatch, socket-path, account, interruption, repair, JSON, and exit cases (`d7230851f`, `151f2ec97`).
- [x] Update the daemon-switch operator skill; prove no implicit switch/restart/restore/daemon invocation (`2017b1291`, `151f2ec97`).
- [x] Run required AY5 gates, push, open the stacked draft PR, report final SHA, and merge-forward the successor worktree (`151f2ec97`, PR #1282).
