# Smoke Fast

- status: `passed`
- timestamp: `2026-05-24T21:16:25.887648+00:00`
- binary SHA: `ae1b753c6931696af7cc4a84aab77a07dad4541f`
- duration secs: `0.905`
- summary: `pass=7`, `fail=0`, `skip=0`

| Row | Verdict | Notes |
| --- | --- | --- |
| `Z1-001` | `PASS` | release smoke binaries built successfully |
| `Z1-002` | `PASS` | doctor auto-started the daemon and reported healthy readiness on the clean-room baseline |
| `Z1-003` | `PASS` | teams and members returned the retained clean-room roster after explicit add-member setup |
| `Z1-004` | `PASS` | list/read/clear/log snapshot all succeeded on the clean-room empty-mailbox baseline |
| `Z1-005` | `PASS` | both send modes succeeded; the ack-required message was read from the recipient mailbox and acknowledged successfully |
| `FAST-LOG-001` | `PASS` | retained log captured send/read/ack/shutdown plus nudge and ack-reply delivery-policy events |
| `FAST-LOG-002` | `PASS` | retained log contained no warning or error records during the healthy fast smoke run |
