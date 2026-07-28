# Hermes as an ATM client

This document describes the supported exploratory use case in which a Python
agent (Hermes) acts as a native ATM client through the PyO3 bindings in
`crates/atm-graft-python`. Hermes uses the typed `atm-graft` client in-process;
it does not shell out to the `atm` CLI and does not open a second daemon or
storage path.

## Integration shape

Each Hermes gateway runs one background `HermesGraftBridge`. At gateway startup
it constructs a `PyGraftSession` through `PyGraftSessionOptions`, scoped to the
gateway's agent and team, then activates the receiver:

```python
caller = atm_graft.PyAgentAddress(
    os.environ["ATM_IDENTITY"],
    os.environ["ATM_TEAM"],
    hermes_chat_id,
)
options = atm_graft.PyGraftSessionOptions(
    os.environ["ATM_HOME"],
    os.environ["ATM_IDENTITY"],
    os.environ["ATM_TEAM"],
)
bridge = HermesGraftBridge(caller, options, inject_user_message)
bridge.start()
```

The receiver is registered by agent/team (`skillrx@hermes`, for example).
The optional caller `chat_id` supplies the Hermes/Telegram conversation context
for client operations; it is not a separate receiver endpoint. Keep the bridge
alive for the gateway lifetime and call `bridge.close()` during shutdown.

The receiver callback runs from the graft receiver thread. The Hermes adapter
maps the canonical source address to an isolated ATM chat key and schedules a
native internal message on the gateway event loop:

```text
PyNudge(source=hendrix:1234@hermes, body=...) →
atm:hendrix:1234@hermes → MessageEvent(internal=True) → gateway._handle_message()
```

The callback uses `asyncio.run_coroutine_threadsafe()` (or the equivalent
gateway-safe scheduling primitive) to cross from the receiver thread into the
Hermes event loop. Duplicate message IDs are suppressed by the bridge. The
body is then handled by Hermes's ordinary inbound-user-message path, so no
manual `atm read` turn is required.

## Required setup

The gateway environment supplies:

- `ATM_IDENTITY` — the agent name;
- `ATM_TEAM` — the ATM team name;
- `ATM_HOME` — the ATM home/workspace root; and
- the Hermes-side chat ID, such as the Telegram chat ID.

The ATM roster must also identify the workspace root where the gateway's graft
endpoint is published. If the gateway profile's `ATM_HOME` differs from its
workspace, set the durable roster metadata explicitly:

```sh
ATM_TEAM=hermes ATM_IDENTITY=hendrix \
  atm teams update-member hermes skillrx \
  --workspace-root /path/to/skillrx/workspace
```

The daemon uses this `workspace_root` metadata (falling back to `home_dir` for
older roster rows), so it resolves the same endpoint path that the Python
publisher writes.

The workspace must contain a discovered `.atm.toml`. Graft activation is
configuration-gated: without that file the session remains `inactive`, even
though graft is enabled by default. A minimal configuration is enough:

```toml
[atm]
```

Use `[atm.graft] enabled = false` only when the receiver should be disabled.
The daemon must be available through the normal same-host bootstrap path; the
Hermes bridge does not start or own `atm-daemon`.

## Build and packaging

Build the Python binding with Maturin from the repository root:

```sh
maturin develop --manifest-path crates/atm-graft-python/Cargo.toml
```

Maturin requires the Python project version to be valid PEP 440. Hyphenated
Cargo prerelease strings such as `1.4.0-beta-ai` cannot be used as the Python
package version. The tested package metadata uses `1.4.0`, while the daemon
workspace may retain its own release string; runtime compatibility is based on
the HTTP API/schema contract rather than crate-version equality.

See the open findings [AI18-GRAFT-PYTHON-NOT-BUILDABLE](../../.triage/phase-AI/findings/AI18-GRAFT-PYTHON-NOT-BUILDABLE.ttl)
and [AI18-GRAFT-PYTHON-BINDING-CONTRACT](../../.triage/phase-AI/findings/AI18-GRAFT-PYTHON-BINDING-CONTRACT.ttl)
for the packaging and exception-contract details. This use-case document does
not close either finding.

## Live validation

The live partner test was run by `skillrx@hermes` against
`testing/hermes-atm-graft` (version fix `b97ab1f3`; the current branch also
contains the runbook note in `5f9db51b`). The observed flow was:

1. Construct the Python session and activate the receiver; the snapshot reached
   `listening`.
2. Perform the daemon-backed ATM write/read flow. A nudge from
   `hendrix:1234@hermes` to the skillrx gateway reached the callback as
   `atm:hendrix:1234@hermes` with the expected body.
3. Delivering the same message ID again produced no second Hermes turn.
4. Close the bridge; receiver shutdown completed cleanly.

Maturin built the `atm_graft-1.4.0` wheel successfully. The focused reference
tests exercised the bridge behavior; six of seven passed. The remaining test
is a known contract mismatch: it expects Python `RuntimeError` for malformed
`hendrix:bad`, while the binding intentionally exposes the structured
`atm_graft.AtmGraftError`. That is tracked by the binding-contract finding
linked above, not by the live receive path.

This is an integration reference, not a sprint status record or AI.21 evidence
claim.
