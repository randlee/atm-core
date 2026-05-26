# Smoke Fast

- status: `passed`
- timestamp: `2026-05-26T02:13:48.634198+00:00`
- binary SHA: `84051c1b278dd10b97a1e13b11f476cd4013fc97`
- duration secs: `0.862`
- summary: `pass=7`, `fail=0`, `skip=0`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `Z1-001` | build approved smoke baseline | `PASS` | release smoke binaries built successfully |
| `Z1-002` | clean-room daemon/runtime bring-up | `PASS` | doctor auto-started the daemon and reported healthy readiness on the clean-room baseline |
| `Z1-003` | retained team/member inspection on clean-room baseline | `PASS` | teams and members returned the retained clean-room roster after explicit add-member setup |
| `Z1-004` | empty-mailbox retained CLI surface | `PASS` | list/read/clear/log snapshot all succeeded on the clean-room empty-mailbox baseline |
| `Z1-005` | first clean-room send to config-defined recipient | `PASS` | both send modes succeeded; the ack-required message was read from the recipient mailbox and acknowledged successfully |
| `FAST-LOG-001` | expected happy-path retained events are present | `PASS` | retained log captured send/read/ack/shutdown plus nudge and ack-reply delivery-policy events |
| `FAST-LOG-002` | retained logs contain no warnings or errors | `PASS` | retained log contained no warning or error records during the healthy fast smoke run |
