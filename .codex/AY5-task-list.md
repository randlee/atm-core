# AY5 — Transactional Herdr entry control plane

- [ ] Define REQ-P-DAEMON-SWITCH-002 and the ADR-053 entry-management addendum.
- [ ] Add native `atm doctor --json` ingestion and validated endpoint model.
- [ ] Implement deterministic identifiers, canonical platform definitions, marker/digest checks, and exactly-one-object JSON output.
- [ ] Implement durable install/remove journals and fail-closed status repair using platform fakes.
- [ ] Add macOS, Linux, and Windows fixture coverage for normal, foreign, mismatch, socket-path, account, interruption, repair, JSON, and exit cases.
- [ ] Update the daemon-switch operator skill; prove no implicit switch/restart/restore/daemon invocation.
- [ ] Run required AY5 gates, push, open the stacked draft PR, report final SHA, and merge-forward the successor worktree.
