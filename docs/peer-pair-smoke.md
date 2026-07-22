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
    {"id": "preflight", "expect": "success", "message_ulid": "<ulid>", "command": ["..."]},
    {"id": "local_smoke", "expect": "success", "message_ulid": "<ulid>", "command": ["..."]},
    {"id": "send_read_nudge", "expect": "success", "message_ulid": "<ulid>", "command": ["..."]},
    {"id": "reverse_send_read_nudge", "expect": "success", "message_ulid": "<ulid>", "command": ["..."]},
    {"id": "requires_ack_reply", "expect": "success", "message_ulid": "<ulid>", "command": ["..."]},
    {"id": "duplicate_ulid", "expect": "success", "message_ulid": "<ulid>", "command": ["..."]},
    {"id": "unavailable_peer", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["..."]},
    {"id": "untrusted_or_allowlist_rejection", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["..."]},
    {"id": "failed_remote_ack", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["..."]}
  ]
}
```

Use only public `atm` or graft-facing daemon-client commands in `command`.
Each successful case must verify its stated semantic result: receiver-visible
read/nudge, same-ULID duplicate with no repeat side effect, or a typed error
without acknowledgement mutation. The runner writes sanitized JSON evidence
with commands, daemon/client versions, trust identity, result, daemon-log window,
role, identities, ULID, transport, and teardown.

The runner stops only its optional `launch_command` child. It waits for that
recorded PID, checks the configured TCP endpoint is closed, then removes only
explicit file/socket paths under a new runtime directory it marked as owned.
Never list an ambient daemon's lock, socket, endpoint, or PID in this
configuration.
