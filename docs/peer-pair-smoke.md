# Peer-pair release smoke

The canonical operator entry point is `just smoke`. The Python modules named
below are implementation details and must not be invoked directly; the routed
commands are documented in [Smoke testing](./smoke-testing.md).

Run this procedure for every release that changes daemon, HTTP, TLS, storage
write, acknowledgement, or peer-transport code. It proves ATM message handling;
a raw TCP connection is not evidence of success.

## Progressive live-daemon commands

Use the matched branch CLI and daemon selected by `daemon-switch`; these
commands never start or stop a daemon themselves. Each command includes the
rows from the preceding command and writes JSON plus an XHTML evidence pane
under `reports/smoke/<feature>/`.

```bash
just smoke localhost
just smoke local-ip
just smoke peer-preflight m5 fastpc4
just smoke crosshost-send m5 fastpc4
just smoke crosshost-ack m5 fastpc4
just smoke crosshost-curl-tls m5 fastpc4
```

`localhost` proves host-qualified localhost send/read and requires-ack/ack.
`local-ip` adds the daemon's advertised-IP route. `peer-preflight` confirms
that each named SSH host already has a healthy, version-matched daemon and an
enabled advertised host; it never starts, stops, or retries a remote daemon.
`crosshost-send` then proves local public `atm send` and remote public `atm
read` return the exact same ULID and body. `crosshost-ack` repeats that proof
with `--requires-ack`, has the remote peer acknowledge it, and proves the
reply reaches the local inbox with the original acknowledged-message ID.
The crosshost-curl-tls lane proves the public mTLS doctor route in both
directions, then repeats each direction without a client certificate. The
negative rows pass only when curl exits nonzero with HTTP status 000, proving
the TLS handshake rejected the caller before Hyper or the ATM router received
HTTP.

Historical cross-host artifacts that predate AO.4 and do not record
`peer_wire_security` are not mode-specific evidence. They may remain useful
for their original transport proof, but never establish AO.4 plaintext or
mTLS behavior, performance, or authentication coverage.

The legacy `just smoke crosshost <host...>` spelling remains an alias for
`crosshost-send`. A cross-host recovery smoke is intentionally not claimed
until the public unconfirmed-send contract exposes the locally persisted
message ULID; otherwise a script could not truthfully prove that recovery
preserved the sender's original ULID.

Each participating host runs the same repository smoke feature with its own
config:

```bash
just smoke peer-pair \
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
    {"id": "untrusted_or_allowlist_rejection", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["atm", "send", "..."], "verification": {"assertions": {"rejected_before_routing": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "count", "equals": 0}}, "required_daemon_log_events": [{"action": "request", "outcome": "rejected", "message": "HTTPS peer request was rejected before or during shared API routing", "fields": {"subsystem": "https_transport"}}], "forbidden_daemon_log_events": [{"action": "peer_delivery", "outcome": "write_persisted"}]}},
    {"id": "failed_remote_ack", "expect": "typed_error", "typed_error_code": "<code>", "message_ulid": "<ulid>", "command": ["atm", "ack", "..."], "verification": {"assertions": {"ack_source_unchanged": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "message.requires_ack", "equals": true}, "no_remote_ack_state": {"command": ["atm", "read", "--message-id", "<ulid>", "--json"], "json_path": "message.acknowledgedAt", "absent": true}}}}
  ]
}
```

Every case command and every assertion command must invoke a public `atm` or
`atm-graft` client; the runner rejects raw sockets, storage helpers, and other
executables. Each `verification.assertions` entry runs its command, reads one
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

Daemon log assertions use exact structured event selectors against the retained
JSONL delta. A selector may match top-level `action`, `outcome`, `message`,
`level`, `service`, or `target` fields and scalar values under `fields`; it is
not a free-form substring search. The untrusted/allowlist case requires the
real HTTPS rejection event (`action=request`, `outcome=rejected`,
`fields.subsystem=https_transport`) to appear after the command. Optional
`forbidden_daemon_log_events` selectors must not appear. Rotation, truncation,
malformed JSONL, or a missing required event fails the case and is recorded in
the evidence with an actionable message.

The runner accepts `launch_command` only with non-empty
`owned_runtime_paths`. It records the launched PID in its ownership marker,
waits for that PID, checks the configured TCP endpoint is closed, then removes
only explicitly listed file/socket paths beneath that marked runtime directory.
Without the matching marker it fails closed and deletes nothing. Never list an
ambient daemon's lock, socket, endpoint, or PID in this configuration.

## Fast inbound two-peer smoke

For live diagnosis against one already-running local daemon, use the inbound
runner. It does not start, stop, switch, configure, or restart any daemon.
It SSHes to each configured peer, runs peer doctor, then has that peer send a
no-ack and an acknowledgement-required message to the local host. The local
host reads each exact returned message ID; the acknowledgement-required row
must remain pending. It writes bounded, sanitized local and remote log tails
after every run.

Copy, then keep the real configuration untracked because it contains machine
addresses and paths:

```bash
cp scripts/smoke/inbound-peer-smoke.example.json inbound-peer-smoke.json
just smoke inbound-peer \
  --config inbound-peer-smoke.json \
  --evidence-dir artifacts/peer-smoke/inbound
```

When diagnosing the explicit `plaintext-test` profile, bind every enabled peer
interface only to the private test overlay address used by the participating
hosts. The profile disables TLS, certificate pinning, and the peer allowlist;
it is never safe to bind it to a public or shared network interface. Restart
without `--peer-wire-security plaintext-test` before any non-diagnostic use.
It remains the ordinary direct-peer HTTP pipeline; its evidence must be labeled
`plaintext-test` and never used to claim mTLS or peer-allowlist coverage (ADR-047).

The runner prints one `PASS` or `FAIL` line for local doctor, each peer doctor,
each peer send/read pair, and the evidence path. It exits zero only when every
row passed. `peers[].shell` is `posix` for a macOS/Linux SSH shell or
`powershell` for Windows; all commands remain argv arrays, so no local shell
or ad-hoc remote script is required. Set `local.advertised_host` explicitly to
avoid interface-discovery ambiguity, or omit it to query
`atm peer interface list --json`.

Every run also writes one valid standalone XHTML pane per computer:
`local.xhtml`, `m5.xhtml`, and so on. Each shows doctor-derived daemon/version
information, the full fixed smoke matrix, executed-session entries, and a
bottom assessment. Rows not covered by this narrowly scoped inbound runner are
explicitly `— not-run`; they are never represented as passing.

### One invocation per host and combined review

Do not make the Mac SSH runner a substitute for host evidence. Run this command
once on each computer using its host-supplied config; a peer's `outbound_target`
is the Mac's qualified address, so M5 and fastpc4 perform their own outbound
sends and publish their own `handoff.json` and XHTML pane:

```bash
just smoke inbound-peer --host \
  --config inbound-peer-smoke.json --evidence-dir artifacts/peer-smoke/inbound
```

After copying or pulling each peer's handoff file to the Mac, run the Mac
invocation with each exact-ID handoff. It does not search mailboxes by message
content: it polls only the exported IDs and requires the ack-required item to
be pending before writing the Mac pane.

```bash
just smoke inbound-peer --host \
  --config inbound-peer-smoke.json --evidence-dir artifacts/peer-smoke/inbound \
  --handoff artifacts/peer-smoke/collected/m5-handoff.json \
  --handoff artifacts/peer-smoke/collected/fastpc4-handoff.json
```

Each host pushes its timestamped evidence directory. After pulling the three
current panes to one directory, the Mac combines them through the repository
`sc-compose` template (and refuses an absent, wrongly labelled, malformed, or
older-than-30-minute pane):

```bash
just smoke inbound-peer-combine \
  --panes-dir artifacts/peer-smoke/collected \
  --hosts local,m5,fastpc4 \
  --output artifacts/peer-smoke/collected/review.xhtml
```
