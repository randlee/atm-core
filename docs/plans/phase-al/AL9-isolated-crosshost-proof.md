# AL.9 isolated plain-TCP cross-host proof

This procedure is evidence-only. It does not authorize daemon activation,
switching, stopping, or use of an ambient account's data. The release operator
must use a clean OS user (or approved equivalent isolated runtime root) on both
hosts and retain the emitted JSON artifact.

## Required replacement configuration

On the receiver, configure the replacement daemon before it starts:

```sh
export ATM_HTTP_DIRECT_PEER_BIND='192.0.2.20:43101'
export ATM_HTTP_DIRECT_PEER_SOURCE_HOST='m5-proof.example.test'
```

On the sender, configure the host-qualified CLI/graft connector:

```sh
export ATM_HTTP_DIRECT_PEER_PORT='43101'
```

The receiver's source host is an exact, adapter-owned identity. The listener
does not derive it from JSON or a socket address. Plain TCP is the AL.9 MVP;
TLS is neither built nor required by this proof.

## Preflight contract

Each side supplies a replacement-only `preflight_command` that prints exactly:

```json
{
  "replacement_runtime": "atm-http-runtime",
  "revision": "<the same full AL.9 SHA on both hosts>",
  "route_evidence": "...",
  "storage_evidence": "...",
  "hook_evidence": "..."
}
```

The runner refuses missing, different, or non-`atm-http-runtime` revisions
before it sends a message. The preflight command must not inspect or control an
ambient daemon.

## Example proof configuration

```json
{
  "schema_version": 1,
  "recipient": "receiver@atm-dev.receiver.example.test",
  "sender": {
    "ssh_command": ["ssh", "m5"],
    "atm_command": ["/clean/al9/bin/atm"],
    "preflight_command": ["/clean/al9/bin/verify-atm-http-runtime"],
    "revision": "<AL9_SHA>",
    "identity": "sender",
    "team": "atm-dev",
    "environment": {
      "ATM_HOME": "/clean/al9/sender-home",
      "ATM_HTTP_DIRECT_PEER_PORT": "43101"
    }
  },
  "receiver": {
    "atm_command": ["/clean/al9/bin/atm"],
    "preflight_command": ["/clean/al9/bin/verify-atm-http-runtime"],
    "revision": "<AL9_SHA>",
    "identity": "receiver",
    "team": "atm-dev",
    "environment": {
      "ATM_HOME": "/clean/al9/receiver-home",
      "ATM_HTTP_DIRECT_PEER_BIND": "192.0.2.20:43101",
      "ATM_HTTP_DIRECT_PEER_SOURCE_HOST": "m5-proof.example.test"
    }
  }
}
```

Run it only after the release operator has approved the isolated hosts:

```sh
python3 scripts/smoke/run_al9_isolated_crosshost.py proof.json \
  --body "al9-direct-crosshost-$(date +%s)"
```

The sender command is one `atm send`; the receiver command is one exact-ID
`atm read`. The JSON evidence records the route, durable storage, and received
hook claims supplied by the replacement preflight. A failure means park the AL
line, leave legacy active, and obtain a new approved proof round.
