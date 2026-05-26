# Smoke

- status: `passed`
- timestamp: `2026-05-26T16:17:52.512750+00:00`
- binary SHA: `84935774c720e06a9e5ae36b9c6073f2231450c2`
- duration secs: `1.352`
- summary: `pass=8`, `fail=0`, `skip=0`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `Z1-001` | build approved smoke baseline | `PASS` | release smoke binaries built successfully |
| `Z1-002` | clean-room daemon/runtime bring-up | `PASS` | doctor auto-started the daemon and reported healthy readiness on the clean-room baseline |
| `Z1-003` | retained team/member inspection on clean-room baseline | `PASS` | teams and members returned the retained clean-room roster after explicit add-member setup |
| `Z1-004` | empty-mailbox retained CLI surface | `PASS` | list/read/clear/log snapshot all succeeded on the clean-room empty-mailbox baseline |
| `Z1-005` | first clean-room send to config-defined recipient | `PASS` | both send modes succeeded; the ack-required message was read from the recipient mailbox and acknowledged successfully |
| `Z1-007` | retained CLI validation and recovery guidance | `PASS` | pending-ack inspection, post-ack mailbox clear/re-read, log snapshot, and invalid-ack recovery guidance all behaved as expected |
| `FAST-LOG-001` | expected happy-path retained events are present | `PASS` | retained log captured send/read/ack/shutdown plus nudge and ack-reply delivery-policy events |
| `FAST-LOG-002` | retained logs contain no warnings or errors | `PASS` | retained log contained no warning records and no unexpected error records during the healthy smoke path |
