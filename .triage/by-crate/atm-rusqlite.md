# atm-rusqlite — Active Findings Index

Crate path: `crates/atm-rusqlite/`

## Open Findings

| ID | Severity | File | Summary | Status |
|----|----------|------|---------|--------|
| RULE-005 | BLOCKING | retired peer transport | Duplicate replay DTO | closed (peer transport deleted) |

## Notes
- Primary concern: boundary violations (atm-daemon or atm importing directly instead of through composition root)
- ATM-QA-009 finding class: `use atm_rusqlite` in peer_transport.rs (R.16) was a blocker — must inject via composition root
- Watch for: direct SQLite imports in non-rusqlite crates
