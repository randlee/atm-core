# Smoke Thorough

- status: `passed`
- timestamp: `2026-05-24T21:35:21.384100+00:00`
- binary SHA: `63e9edc83b7b278c554387ccf92c1c8187f9036a`
- duration secs: `1.448`
- summary: `pass=11`, `fail=0`, `skip=0`

| Row | Verdict | Notes |
| --- | --- | --- |
| `Z1-001` | `PASS` | release smoke binaries built successfully |
| `Z1-002` | `PASS` | doctor auto-started the daemon and reported healthy readiness on the clean-room baseline |
| `Z1-003` | `PASS` | teams, members, backup, and restore dry-run all succeeded on the clean-room retained/admin baseline |
| `Z1-004` | `PASS` | list/read/clear/log snapshot plus ATM help overview/send guidance all succeeded on the clean-room baseline |
| `Z1-005` | `PASS` | both send modes, pending-ack inspection, recipient read/ack, and post-ack clear/re-read all succeeded on the clean-room baseline |
| `Z1-006` | `PASS` | copied-state durable send succeeded and surfaced the compatibility append degraded warning after the legacy-array inbox projection failed |
| `Z1-007` | `PASS` | send/read/ack/list/clear/log/doctor/teams/members/help common error paths all failed closed with explicit actionable guidance |
| `Z1-008` | `PASS` | disposable copied-state doctor/list/send/read all succeeded without touching live host ATM state |
| `Z1-009` | `PASS` | copied-state log snapshot retained the expected retry-visible daemon lifecycle outcomes while the durable send/read path succeeded |
| `FAST-LOG-001` | `PASS` | retained log captured send/read/ack/shutdown plus nudge and ack-reply delivery-policy events before negative-path execution |
| `FAST-LOG-002` | `PASS` | retained log contained no warning or error records during the healthy-path portion of the thorough run |
