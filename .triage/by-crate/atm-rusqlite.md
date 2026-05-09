# atm-rusqlite — Active Findings Index

Crate path: `crates/atm-rusqlite/`

## Open Findings

| ID | Severity | File | Summary | Status |
|----|----------|------|---------|--------|
| RULE-005 | BLOCKING | src/lib.rs:30 | RemoteReplayStateRecord duplicate struct (canonical here, duplicate in atm-daemon/peer_transport.rs:74) | open |

## Notes
- Primary concern: boundary violations (atm-daemon or atm importing directly instead of through composition root)
- ATM-QA-009 finding class: `use atm_rusqlite` in peer_transport.rs (R.16) was a blocker — must inject via composition root
- Watch for: direct SQLite imports in non-rusqlite crates
