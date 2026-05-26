# Smoke Thorough

- status: `passed`
- timestamp: `2026-05-26T02:14:58.877354+00:00`
- binary SHA: `84051c1b278dd10b97a1e13b11f476cd4013fc97`
- duration secs: `1.664`
- summary: `pass=12`, `fail=0`, `skip=0`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `Z1-001` | build approved smoke baseline | `PASS` | release smoke binaries built successfully |
| `Z1-002` | clean-room daemon/runtime bring-up | `PASS` | doctor auto-started the daemon and reported healthy readiness on the clean-room baseline |
| `Z1-003` | retained team/member inspection on clean-room baseline | `PASS` | teams, members, backup, and restore dry-run all succeeded on the clean-room retained/admin baseline |
| `Z1-004` | empty-mailbox retained CLI surface | `PASS` | list/read/clear/log snapshot plus ATM help overview/send guidance all succeeded on the clean-room baseline |
| `Z1-005` | first clean-room send to config-defined recipient | `PASS` | both send modes, pending-ack inspection, recipient read/ack, and post-ack clear/re-read all succeeded on the clean-room baseline |
| `Z1-006` | degraded notification after durable send | `PASS` | copied-state durable send succeeded and surfaced the compatibility append degraded warning after the legacy-array inbox projection failed |
| `Z1-007` | retained CLI validation and recovery guidance | `PASS` | send/read/ack/list/clear/log/doctor/teams/members/help common error paths all failed closed with explicit actionable guidance |
| `Z1-008` | copied-state durable baseline bring-up | `PASS` | disposable copied-state doctor/list/send/read all succeeded without touching live host ATM state |
| `Z1-009` | reconcile/runtime retry-visible smoke coverage | `PASS` | copied-state log snapshot retained the expected retry-visible daemon lifecycle outcomes while the durable send/read path succeeded |
| `PRR-001` | shared-host multi-workspace same-daemon smoke coverage | `PASS` | two workspaces with one shared ATM_HOME daemon/database/log root handled concurrent send/read/ack traffic without cross-workspace leakage |
| `FAST-LOG-001` | expected happy-path retained events are present | `PASS` | retained log captured send/read/ack/shutdown plus nudge and ack-reply delivery-policy events before negative-path execution |
| `FAST-LOG-002` | retained logs contain no warnings or errors | `PASS` | retained log contained no warning or error records during the healthy-path portion of the thorough run |
