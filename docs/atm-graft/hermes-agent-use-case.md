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

The receiver is registered by agent/team (`receiver@team`, for example).
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
PyNudge(source=sender:chat-id@team, body=...) →
AtmGraftAdapter → session.steer(runtime_session_id, body)
```

The adapter uses `asyncio.run_coroutine_threadsafe()` (or the equivalent
gateway-safe scheduling primitive) to cross from the receiver thread into the
Hermes event loop. Duplicate message IDs are suppressed by the bridge. A
missing/invalid runtime binding or rejected steer is surfaced as a typed
failure; there is no normal-message fallback or retry queue. The bridge never
stores or consumes mail itself.

For messages that require an explicit ATM acknowledgement, pass the optional
`requires_ack` argument and acknowledge the returned message after reading it:

```python
sender_session.send(receiver, "please confirm", requires_ack=True)
message = next(
    message for message in receiver_session.read() if message.body == "please confirm"
)
receiver_session.acknowledge(message.message_id, "confirmed")
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
ATM_TEAM=<team> ATM_IDENTITY=<caller-identity> \
  atm teams update-member <team> <receiver-identity> \
  --workspace-root <workspace-root>
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

## Installed Hermes gateway setup

Use this deployment order exactly:

1. Upgrade the Hermes host to the released version from the
   `randlee/hermes-agent` fork that supplies the public startup and injection
   capability required by `hermes-atm`.
2. Configure the Hermes profile environment and LaunchAgent identity.
3. Install immutable `atm-graft` and `hermes-atm` wheels in the **same Python
   interpreter named by that profile's LaunchAgent**.
4. Run the package-owned `hermes-atm install` command to materialize the
   declarative profile hook.
5. Reset the named gateway through its supported lifecycle command.

Do not use an editable ATM checkout, modify Hermes source, create the receiver
endpoint yourself, or copy a custom handler into a profile. The package
installer validates the public gateway capability and materializes the
standard declarative startup hook.

All values below are profile-local deployment inputs. Keep them out of source,
commit messages, fixtures, logs, and shared evidence.

```sh
GATEWAY_PY=<launch-agent-python-from-randlee-hermes-agent>
PROFILE_HOME=<profile-home>
PLIST=<launch-agent-plist>

"$GATEWAY_PY" -m pip install 'atm-graft==<released-atm-graft-version>' \
  'hermes-atm==<released-hermes-atm-version>'

"$GATEWAY_PY" -m hermes_atm install \
  --profile <profile> \
  --profile-home "$PROFILE_HOME" \
  --identity <atm-identity> \
  --team <atm-team> \
  --chat-id <telegram-chat-id> \
  --atm-home <atm-home> \
  --workspace-root <workspace-root> \
  --launch-agent-plist "$PLIST"
```

Restart the named gateway through its supported lifecycle command. On
`gateway:startup`, the installed hook starts the profile-owned Graft receiver
and publishes the ordinary schema-v2 endpoint under
`<profile-home>/.atm/graft/<atm-team>/<atm-identity>.json`. Do not create or
edit that record manually.

The first live gate is a profile-local localhost self-send. Run it from the
configured Hermes profile only after the endpoint is published. It must
produce one successful receiver delivery and inject one host-originated nudge
into that profile's existing Hermes context; the normal agent response must
then follow without any out-of-band instruction to read ATM.

For valid nudge proof, require all of: successful receiver-hook delivery from
the localhost send, the visible host notice in the configured existing session,
an ordinary recipient response, and the resulting ATM acknowledgement. A
manual `atm read` request proves only mailbox persistence, not nudge delivery.

## Historical validation (pre-AI.38)

The following retained evidence predates the shipped AI.36–AI.38 steer
contract. It is kept for provenance only and must not be used as an operator
recipe or as evidence for the current production path. The observed pre-steer
flow was:

1. Construct the Python session and activate the receiver; the snapshot reached
   `listening`.
2. Perform the daemon-backed ATM write/read flow. A nudge from a registered
   sender reached the receiver callback with the expected body.
3. Delivering the same message ID again produced no second Hermes turn.
4. Close the bridge; receiver shutdown completed cleanly.

Maturin built the `atm_graft-1.4.0` wheel successfully. The focused reference
tests exercised the bridge behavior; six of seven passed. The remaining test
is a known contract mismatch: it expects Python `RuntimeError` for malformed
`sender:bad`, while the binding intentionally exposes the structured
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
one registered sender as the source, so both identities must be
present in the same team roster. Do not invoke the underlying Python module
directly:

```sh
ATM_IDENTITY=<receiver-identity> ATM_TEAM=<team> \
  just smoke graft-hermes \
  --sender <registered-sender> \
  --workspace-root <workspace-root> \
  --chat-id <telegram-chat-id>
```

The test covers:

- `PyAgentAddress` and `PyGraftSessionOptions` construction and validation;
- `PyGraftSession` activation, duplicate activation rejection, snapshot, and
  close lifecycle;
- daemon-backed `send`, `read`, and `acknowledge` operations;
- typed `PyNudge` callback delivery with source, body, and message ID checks;
- typed `PyMessage` read projections and acknowledgement reply; and
- post-close fail-closed behavior.

The receiver endpoint must be published while the test runs. If the test
reports `ATM_POST_SEND_GRAFT_UNAVAILABLE`, inspect the gateway hook and the
profile's `.atm/graft/<team>/<agent>.json` endpoint before rerunning QA.
