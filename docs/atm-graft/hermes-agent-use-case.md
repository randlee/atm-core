# Hermes as an ATM client

This document describes the supported exploratory use case in which a Python
agent (Hermes) acts as a native ATM client through the PyO3 bindings in
`crates/atm-graft-python`. Hermes uses the typed `atm-graft` client in-process;
it does not shell out to the `atm` CLI and does not open a second daemon or
storage path.

## Status

**Current shipped flow (AI.36–AI.38):** one lease-safe receiver per profile
identity, one ten-second durable-work summary after recovery when the receiver
is listening, and an adapter that injects live/recovery wake-ups through the
configured Hermes steer path. The production composition seam is
`atm_graft_hermes_loader.HermesGraftRuntime`; the Hermes gateway supplies its
authenticated RPC request function and registration-backed runtime-session
resolver.

## Integration shape

Each Hermes gateway runs one background `HermesGraftRuntime`. At gateway startup
the loader validates the ATM profile environment, constructs a `PyGraftSession`
through `PyGraftSessionOptions`, and binds the authenticated Hermes RPC and
registration resolver:

```python
from atm_graft_hermes_loader import HermesGraftRuntime

runtime = HermesGraftRuntime.from_environment(
    request=authenticated_hermes_request,
    resolve_session_id=hermes_registration.resolve_session_id,
)
await runtime.start()
```

The receiver is registered by agent/team (`skillrx@hermes`, for example).
The required `ATM_CHAT_ID` supplies the Hermes/Telegram conversation context
for client operations; it is not a separate receiver endpoint. Keep the
runtime alive for the gateway lifetime and call `runtime.close()` during
shutdown.

The receiver callback runs from the graft receiver thread. The loader wires the
typed callback to `AtmGraftAdapter`, which schedules a non-interrupting
`session.steer` on the gateway event loop. The host maps `ATM_CHAT_ID` through
its registration lifecycle to the opaque Hermes runtime session ID; the source
address remains attribution/reply metadata only:

```text
PyNudge(source=hendrix:1234@hermes, body=...) →
AtmGraftAdapter → session.steer(runtime_session_id, body)
```

The adapter uses `asyncio.run_coroutine_threadsafe()` (or the equivalent
gateway-safe scheduling primitive) to cross from the receiver thread into the
Hermes event loop. Duplicate message IDs are suppressed by the bridge. A
missing/invalid runtime binding or rejected steer is surfaced as a typed
failure; there is no normal-message fallback or retry queue. The bridge never
stores or consumes mail itself.

For messages that require an explicit ATM acknowledgement, pass the optional
`requires_ack` argument. The Python graft client can send and read in-process;
acknowledgement remains the native `atm ack` CLI workflow, which emits the
canonical linked write for the selected message:

```python
sender_session.send(receiver, "please confirm", requires_ack=True)
message = next(
    message for message in receiver_session.read() if message.body == "please confirm"
)
# Then use: atm ack <message-id> "confirmed"
```

The default remains `False`, so existing `send(to, body)` callers retain the
non-acknowledgement semantics.

## Required setup

The gateway environment supplies:

- `ATM_IDENTITY` — the agent name;
- `ATM_TEAM` — the ATM team name;
- `ATM_HOME` — the ATM home/workspace root; and
- `ATM_CHAT_ID` — the Telegram chat ID that identifies the profile's current
  host session. The loader requires this value and never sends it as a Hermes
  runtime `session_id`; the host resolver maps it to the opaque ID returned by
  Hermes registration.

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

## Historical validation (pre-AI.38)

The following retained evidence predates the shipped AI.36–AI.38 steer
contract. It is kept for provenance only and must not be used as an operator
recipe or as evidence for the current production path. The live partner test
was run by `skillrx@hermes` against `testing/hermes-atm-graft` (version fix
`b97ab1f3`; the branch also contained the runbook note in `5f9db51b`). The
observed pre-steer flow was:

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

This is historical integration evidence, not a current sprint status record or
AI.21 evidence claim. Current operators must use `HermesGraftRuntime` and the
non-interrupting steer path described above.

## Full smoke test

The bridge unit tests validate source-to-chat mapping and duplicate suppression.
For a live end-to-end check of every PyO3 session operation, first build the
binding in the Hermes Python environment:

```sh
maturin develop --manifest-path crates/atm-graft-python/Cargo.toml
```

Then run the canonical smoke feature as the receiving Hermes profile. It uses
the registered `hendrix` member as the sender, so both identities must be
present in the same team roster. Do not invoke the underlying Python module
directly:

```sh
ATM_IDENTITY=skillrx ATM_TEAM=hermes \
  just smoke graft-hermes \
  --sender hendrix \
  --workspace-root /Users/randlee/Documents/github/synaptic-canvas-dolt \
  --chat-id 8991600178
```

The test covers:

- `PyAgentAddress` and `PyGraftSessionOptions` construction and validation;
- `PyGraftSession` activation, duplicate activation rejection, snapshot, and
  close lifecycle;
- daemon-backed in-process `send` and `read` operations;
- typed `PyNudge` callback delivery with source, body, and message ID checks;
- typed `PyMessage` read projections; and
- post-close fail-closed behavior.

The receiver endpoint must be published while the test runs. If the test
reports `ATM_POST_SEND_GRAFT_UNAVAILABLE`, inspect the gateway hook and the
profile's `.atm/graft/<team>/<agent>.json` endpoint before rerunning QA.
