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
    {"id": "preflight", "expect": "success", "message_ulid": "<ulid>", "command": ["atm", "doctor", "--json"], "verification": {"assertions": {"daemon_ready": {"command": ["atm", "doctor", "--json"], "json_path": "runtime_status.readiness", "equals": "ready"}}}},
    {"id": "local_smoke", "expect": "success", "message_ulid": "<ulid>", "command": ["atm", "send", "..."], "verification": {"assertions": {"receiver_visible": {"command": ["atm", "peek", "--message-id", "<ulid>", "--json"], "json_path": "selected_message_id", "equals": "$message_ulid"}}}},
    {"id": "send_read_nudge", "expect": "success", "message_ulid": "<ulid>", "command": ["atm", "send", "..."], "verification": {"assertions": {"receiver_visible": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "selected_message_id", "equals": "$message_ulid"}, "nudge_visible": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "message.message_id", "equals": "$message_ulid"}}}},
    {"id": "reverse_send_read_nudge", "expect": "success", "message_ulid": "<ulid>", "command": ["atm", "send", "..."], "verification": {"assertions": {"receiver_visible": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "selected_message_id", "equals": "$message_ulid"}, "nudge_visible": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "message.message_id", "equals": "$message_ulid"}}}},
    {"id": "requires_ack_reply", "expect": "success", "message_ulid": "<ulid>", "command": ["atm", "send", "..."], "verification": {"assertions": {"ack_reply_visible": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "selected_message_id", "equals": "$message_ulid"}}}},
    {"id": "duplicate_ulid", "expect": "success", "message_ulid": "<ulid>", "command": ["atm", "send", "..."], "verification": {"assertions": {"receiver_visible": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "selected_message_id", "equals": "$message_ulid"}, "single_record_retained": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "match_count", "equals": 1}, "no_repeat_nudge": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "additional_match_count", "equals": 0}, "no_ack_mutation": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "message.acknowledgedAt", "absent": true}}}},
    {"id": "unavailable_peer", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["atm", "send", "..."], "verification": {"assertions": {"no_prohibited_delivery_state": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "count", "equals": 0}}}},
    {"id": "untrusted_or_allowlist_rejection", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["atm", "send", "..."], "verification": {"assertions": {"rejected_before_routing": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "count", "equals": 0}}, "forbidden_daemon_log_entries": ["<configured-routing-attempt-record>"]}},
    {"id": "failed_remote_ack", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["atm", "ack", "..."], "verification": {"assertions": {"ack_source_unchanged": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "message.requires_ack", "equals": true}, "no_remote_ack_state": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "message.acknowledgedAt", "absent": true}}}}
  ]
}
```

Every peer-pair case command and every assertion command must invoke the public
`atm` CLI; the runner rejects raw sockets, storage helpers, and other
executables. `atm-graft` is an embeddable library, not an executable: prove its
local host lane separately with `python scripts/smoke/run_graft_same_host.py`.
Each `verification.assertions` entry runs its command, reads one
dotted JSON path, and compares it to its scalar `equals` value;
`$message_ulid` resolves to that case's ID. The required assertion names are
fixed by case: duplicate requires one retained record and no repeat side effect;
failed remote ack requires unchanged source acknowledgement state; rejection
requires both its state assertion and a daemon-log delta with no routing entry.
Use `"absent": true` instead of `equals` when a false state is represented by
an omitted JSON field (for example `message.acknowledgedAt`).
The runner writes sanitized JSON evidence with transport and semantic results,
daemon/client versions, trust identity, log window, role, identities, ULID, and
teardown.

The runner accepts `launch_command` only with non-empty
`owned_runtime_paths`. It records the launched PID in its ownership marker,
waits for that PID, checks the configured TCP endpoint is closed, then removes
only explicitly listed file/socket paths beneath that marked runtime directory.
Without the matching marker it fails closed and deletes nothing. Never list an
ambient daemon's lock, socket, endpoint, or PID in this configuration.
