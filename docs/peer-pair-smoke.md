# Peer-pair release smoke

Run this procedure for every release that changes daemon, HTTP, TLS, storage
write, acknowledgement, or peer-transport code. It proves ATM message handling;
a raw TCP connection is not evidence of success.

Each participating host runs the same repository runner with its own config:

```bash
python3 scripts/smoke/run_peer_pair.py \
  --config peer-smoke-role-a.json \
  --evidence-dir artifacts/peer-smoke/role-a
```

The config is deliberately host supplied. Do not commit addresses, certificates,
capabilities, or secrets. It has this shape:

```json
{
  "schema_version": 1,
  "role": "A",
  "commit": "<tested-commit>",
  "client_version_command": ["atm", "--version"],
  "peer_security": {
    "trust_id": "<approved-peer-trust-record>",
    "certificate_fingerprint": "<peer-certificate-fingerprint>"
  },
  "daemon": {
    "endpoint": "<host>:43101",
    "version_command": ["atm", "doctor", "--json"],
    "log_file": "<optional-daemon-log-file>",
    "launch_command": ["<optional-runner-owned-daemon-command>"],
    "runtime_dir": "<new-empty-directory-owned-by-the-runner>",
    "owned_runtime_paths": ["<file-or-socket-under-runtime_dir-created-by-launch-command>"]
  },
  "identities": {"sender": "agent-a@team-a", "recipient": "agent-b@team-b"},
  "cases": [
    {"id": "preflight", "expect": "success", "message_ulid": "<ulid>", "command": ["..."], "verification": {"command": ["atm", "doctor", "--json"], "expected_json": {"runtime_status.readiness": "ready"}}},
    {"id": "local_smoke", "expect": "success", "message_ulid": "<ulid>", "command": ["..."], "verification": {"command": ["atm", "peek", "--message-id", "<ulid>", "--json"], "expected_json": {"message_id": "$message_ulid"}}},
    {"id": "send_read_nudge", "expect": "success", "message_ulid": "<ulid>", "command": ["..."], "verification": {"command": ["atm", "peek", "--message-id", "<ulid>", "--json"], "expected_json": {"message_id": "$message_ulid"}}},
    {"id": "reverse_send_read_nudge", "expect": "success", "message_ulid": "<ulid>", "command": ["..."], "verification": {"command": ["atm", "peek", "--message-id", "<ulid>", "--json"], "expected_json": {"message_id": "$message_ulid"}}},
    {"id": "requires_ack_reply", "expect": "success", "message_ulid": "<ulid>", "command": ["..."], "verification": {"command": ["atm", "peek", "--message-id", "<ulid>", "--json"], "expected_json": {"message_id": "$message_ulid"}}},
    {"id": "duplicate_ulid", "expect": "success", "message_ulid": "<ulid>", "command": ["..."], "verification": {"command": ["atm", "peek", "--message-id", "<ulid>", "--json"], "expected_json": {"message_id": "$message_ulid"}, "forbidden_daemon_log_entries": ["duplicate nudge"]}},
    {"id": "unavailable_peer", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["..."], "verification": {"command": ["atm", "peek", "--message-id", "<ulid>", "--json"], "expected_json": {"message_id": "$message_ulid"}}},
    {"id": "untrusted_or_allowlist_rejection", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["..."], "verification": {"command": ["atm", "peek", "--message-id", "<ulid>", "--json"], "expected_json": {"message_id": "$message_ulid"}, "forbidden_daemon_log_entries": ["peer delivery"]}},
    {"id": "failed_remote_ack", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["..."], "verification": {"command": ["atm", "peek", "--message-id", "<ulid>", "--json"], "expected_json": {"message_id": "$message_ulid", "requires_ack": true}}}
  ]
}
```

Every case requires a `verification` object. Its `command` must be a public
`atm` or graft-facing daemon-client command that emits JSON. `expected_json`
maps dotted JSON paths to exact scalar values; `$message_ulid` resolves to that
case's `message_ulid`. This verifies receiver-visible state instead of asking a
case command to invent runner-only metrics. For example, the duplicate case
verifies one receiver record with the original ULID, and failed-ack verifies the
source remains pending. `untrusted_or_allowlist_rejection` additionally requires
`forbidden_daemon_log_entries`: the runner snapshots the configured daemon log
before the command and rejects any listed routing/delivery entry in the new log
window. The runner writes sanitized JSON evidence with commands, daemon/client
versions, trust identity, transport result, semantic verification, daemon-log
window, role, identities, ULID, transport, and teardown.

The runner stops only its optional `launch_command` child. It waits for that
recorded PID, checks the configured TCP endpoint is closed, then removes only
explicit file/socket paths under a new runtime directory it marked as owned.
Never list an ambient daemon's lock, socket, endpoint, or PID in this
configuration.
