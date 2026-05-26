# Smoke

- status: `failed`
- timestamp: `2026-05-26T02:11:27.372028+00:00`
- binary SHA: `84051c1b278dd10b97a1e13b11f476cd4013fc97`
- duration secs: `0.948`
- summary: `pass=7`, `fail=1`, `skip=0`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `Z1-001` | build approved smoke baseline | `PASS` | release smoke binaries built successfully |
| `Z1-002` | clean-room daemon/runtime bring-up | `PASS` | doctor auto-started the daemon and reported healthy readiness on the clean-room baseline |
| `Z1-003` | retained team/member inspection on clean-room baseline | `PASS` | teams and members returned the retained clean-room roster after explicit add-member setup |
| `Z1-004` | empty-mailbox retained CLI surface | `PASS` | list/read/clear/log snapshot all succeeded on the clean-room empty-mailbox baseline |
| `Z1-005` | first clean-room send to config-defined recipient | `PASS` | both send modes succeeded; the ack-required message was read from the recipient mailbox and acknowledged successfully |
| `Z1-007` | retained CLI validation and recovery guidance | `PASS` | pending-ack inspection, post-ack mailbox clear/re-read, log snapshot, and invalid-ack recovery guidance all behaved as expected |
| `FAST-LOG-001` | expected happy-path retained events are present | `PASS` | retained log captured send/read/ack/shutdown plus nudge and ack-reply delivery-policy events |
| `FAST-LOG-002` | retained logs contain no warnings or errors | `FAIL` | retained log severity gate failed |


## Deviations

### `FAST-LOG-002`

- observed: {
  "passed": false,
  "expected_events": [
    "\"action\":\"send\"",
    "\"action\":\"read\"",
    "\"action\":\"ack\"",
    "\"outcome\":\"delivery_policy.new_message.primary_nudge\"",
    "\"outcome\":\"delivery_policy.ack_reply.delivered\"",
    "\"action\":\"shutdown_completed\""
  ],
  "missing_events": [],
  "warning_records": [],
  "error_records": [
    "{\"version\":\"v1\",\"timestamp\":\"2026-05-26T02:11:27.256891Z\",\"level\":\"Error\",\"service\":\"atm\",\"target\":\"atm.command\",\"action\":\"service\",\"message\":\"ATM command atm completed with outcome error\",\"identity\":{\"hostname\":null,\"pid\":null},\"trace\":null,\"request_id\":null,\"correlation_id\":null,\"outcome\":\"error\",\"diagnostic\":null,\"state_transition\":null,\"fields\":{\"agent\":\"z20-recipient\",\"command\":\"atm\",\"dry_run\":false,\"error_code\":\"ATM_MESSAGE_VALIDATION_FAILED\",\"error_message\":\"ATM CLI command failed unexpectedly: invalid message id: \\n  Recovery: Correct the invalid ATM input or mailbox state, then retry the command with a valid target or argument.\",\"requires_ack\":false,\"sender\":\"z20-recipient\",\"team\":\"z20-team\"}}"
  ]
}
- expected: retained log contains no warning or error records on a healthy fast smoke run
- likely root cause: one or more healthy-path events are still being emitted at warn/error severity
- artifact: /var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/z20-team-normal.q8djhzw8/logs/atm.log.jsonl
