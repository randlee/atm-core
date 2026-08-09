# AL.16 — Hermes ATM Graft Live Proof

**branch:** `plan/al16-hermes-graft-live-proof` (this planning branch only)
**implementation base:** a fresh `/sc-git-worktree` from the accepted
`origin/integrate/phase-al` SHA
**owners:** ATM integration owner (architecture/review), Cipher-311d (Python
binding and Hermes coordination), `skillrx@hermes` (Hermes gateway operator)
**goal:** prove a durable ATM write produces one non-interrupting injection
into the intended live Hermes Telegram session.

## The MVP model

`hermes-atm` is the proposed Hermes-facing Python distribution. It depends on
the generic native `atm-graft` wheel; `atm-graft` remains the Rust/PyO3 client
and receiver capability, while `hermes-atm` owns only Hermes composition. This
keeps a Hermes installation explicit:

```text
pip install hermes-atm
```

The current checked-in Maturin project is named `atm-graft` and already ships
the reference modules `atm_graft_hermes_loader`,
`atm_graft_hermes_bridge`, and `atm_graft_hermes_adapter`. It is **not yet** a
separate installable `hermes-atm` distribution and no production Hermes runner
instantiates the loader. AL.16 closes those two gaps without changing ATM
transport, storage, the legacy daemon, or the Telegram gateway protocol.

One Hermes agent profile owns one tuple:

```text
(ATM_HOME, ATM_TEAM, ATM_IDENTITY, ATM_CHAT_ID)
```

For example, `skillrx@hermes` and `hendrix@hermes` use different
`ATM_CHAT_ID` values and therefore different configured host sessions. Each
profile starts one `HermesGraftRuntime`, which owns exactly one graft receiver
for `(canonical graft root, team, agent)`. The receiver's endpoint is selected
by recipient agent/team; the configured `ATM_CHAT_ID` selects that receiving
profile's Telegram session. The sender's source/chat identity is attribution
and reply metadata only. It must never select a Hermes session.

The only host injection is:

```text
durable ATM write
  -> recipient graft receiver callback
  -> AtmGraftAdapter on the gateway event loop
  -> resolve_session_id(ATM_CHAT_ID)
  -> session.steer(runtime_session_id, text)
  -> next safe tool boundary in that exact Telegram session
```

No normal Telegram `MessageEvent`, second mailbox, replay queue, poll loop,
second receiver, or daemon-owned Hermes session is permitted.

## Binding requirements

| Requirement / decision | AL.16 implementation and proof |
| --- | --- |
| `REQ-GRAFT-PYTHON-001`, ADR-039 | `hermes-atm` uses only the existing PyO3 `atm-graft` API. It does not open a socket, access storage, or add a send/read/ack path. |
| `REQ-GRAFT-RUNTIME-002`, ADR-043.1 | One profile starts one generation-owned receiver. The endpoint record belongs to the receiver, never the Telegram gateway port, and restart reclaims only a stale/dead owner. |
| `REQ-GRAFT-NOTIFY-002`, ADR-043.2/6 | Nudge is a bounded wake signal. Failed steer is observable and fails closed; there is no retry, durable graft state, or message replay. |
| `REQ-GRAFT-HERMES-002`, ADR-039, ADR-043.3 | `ATM_CHAT_ID` is required at startup, resolves through the real Hermes registration/rebind map to an opaque runtime session id, and invokes only `session.steer`. Tests prove one profile cannot steer another profile's chat. |
| `REQ-GRAFT-HERMES-003`, ADR-043.4 | After listening, exactly one ten-second count-only recovery summary may steer the configured session. It must not read, acknowledge, mutate, or replay mail. |

## Delivery order

### AL16.1 — Installable host package

1. Create a dedicated `hermes-atm` Python distribution which depends on a
   compatible `atm-graft` wheel. Keep the native extension in `atm-graft`; do
   not copy its Rust source or make a second transport client.
2. Move/re-export the existing loader, bridge, and adapter through that
   distribution with one documented public runtime entry point:
   `HermesGraftRuntime.from_environment(request=..., resolve_session_id=...)`.
   Compatibility re-exports from the existing package may remain only if they
   do not create two implementations.
3. Add an isolated-venv wheel-install test: install `hermes-atm`, import the
   runtime, and show missing `ATM_HOME`, `ATM_IDENTITY`, `ATM_TEAM`, or
   `ATM_CHAT_ID` fails before receiver activation. The test must not need a
   Hermes checkout.
4. Publish the actual wheel name/version/install command in its package
   metadata and operator documentation. Do not claim that the current
   `atm-graft` wheel is already named `hermes-atm`.

#### Required Python compatibility evidence

The native PyO3 extension is interpreter-specific unless its build metadata
explicitly proves an `abi3` wheel. AL.16 must therefore build and install—not
merely import from a source checkout—the matching wheel in both supported
Hermes environments:

| Lane | Interpreter | Purpose |
| --- | --- | --- |
| Hermes/M4 | CPython 3.14 | Current upgraded Hermes deployment. |
| Hermes/M5 | CPython 3.11 | Default Hermes-agent compatibility target. |

For each lane, retain the interpreter path/version, wheel filename/tag,
`pip install hermes-atm` result, `just test-hermes-graft-bridge` result, and
the isolated-import/startup-validation result. A wheel built for one CPython
minor version is never accepted as evidence for the other. If Maturin/PyO3
cannot build the 3.14 lane, treat it as a packaging defect and fix the
supported build configuration; do not silently downgrade Hermes or bypass pip
with `PYTHONPATH`.

### AL16.2 — Real Hermes lifecycle binding

1. In the Hermes integration, construct one `HermesGraftRuntime` per live
   agent profile after that profile's Telegram session registration/rebind is
   complete. The gateway supplies:
   - its authenticated `session.steer` RPC callable; and
   - an async resolver from that profile's `ATM_CHAT_ID` to the current opaque
     Hermes runtime session id.
2. Keep the runtime alive for the gateway/profile lifetime. On shutdown call
   `runtime.close()` exactly once; it closes the graft receiver and recovery
   timer, but does not stop, start, or supervise `atm-daemon`.
3. Verify the published receiver record is schema-current, owns a random
   generation, has a receiver socket path/port rather than the Telegram
   gateway endpoint, and names the intended optional chat id for
   observability. A stale v1 record or a record pointing at the gateway is a
   deployment defect: restart/re-publish through the current runtime. Do not
   hand-edit records or add a permissive v1 fallback.
4. Bind a second test profile with a different `ATM_CHAT_ID`. Prove that its
   resolver is different and a nudge for profile A cannot call profile B's
   `session.steer`.

### AL16.3 — Live Telegram safe-boundary proof

Run this only with a real, approved Hermes gateway/session. `skillrx@hermes`
is the first target; later agents repeat the same profile procedure rather
than adding a multi-chat registry.

1. Build the matching native wheel and run the existing reference gate:

   ```sh
   just test-hermes-graft-bridge
   python3 scripts/phase-ai/run-hermes-steer-smoke.py --fixture
   ```

2. Install the built `hermes-atm` wheel into the Hermes environment, start the
   real profile runtime, and record its exact wheel version, ATM candidate
   SHA, profile identity, and a redacted/session-safe chat-id fingerprint.
3. Wait for a listening receiver and confirm its endpoint record has current
   schema/generation and is owned by the graft runtime process. Confirm the
   local replacement daemon is healthy with `atm doctor --json`.
4. From a distinct registered ATM sender, perform one ordinary ATM write to
   the recipient. Retain the message id, durable recipient read result, and
   the `session.steer` request/result. It passes only when the text appears at
   the recipient Telegram session's next safe tool boundary.
5. Assert all negative properties for that live nudge:
   - it targeted the resolved runtime id for the configured recipient
     `ATM_CHAT_ID`, not the raw chat id or sender chat id;
   - it did not invoke normal user-message ingress or interrupt active work;
   - it did not change/read/ack the mailbox as a side effect;
   - re-delivery of the same ATM message id does not produce a second steer.
6. Restart the profile/runtime with unread or pending-ack durable work. After
   listening, prove exactly one count-only recovery steer occurs after ten
   seconds; prove no individual-message replay or second summary occurs.
7. Exercise ordinary ATM `read` and `ack` separately through graft after the
   wake. The acknowledgement must preserve the normal ATM address/reply
   route; waking does not imply or perform acknowledgement.

## Evidence, fixes, and closure

- Use the existing `just test-hermes-graft-bridge`,
  `run-hermes-steer-smoke.py`, and managed report conventions. Do not add a
  second smoke runner or a hand-maintained endpoint fixture.
- Record a redacted live-proof report and link it from `site/reports`; do not
  commit live chat ids, Hermes credentials, gateway paths, or personal
  profiles.
- Cipher may fix reproducible Python packaging, loader, adapter, or endpoint
  publication defects in a new worktree from `origin/integrate/phase-al`.
  Each fix gets focused tests and a separately reviewed PR. It must not patch
  `crates/atm-daemon` or duplicate HTTP/daemon code.
- A new gateway type, multi-chat fan-out for one ATM profile, retry/replay
  behavior, or a changed Telegram session-security policy requires Rand's
  decision and a follow-up ADR; it is outside AL.16.

## Acceptance

AL.16 is ready to merge only when:

1. `pip install hermes-atm` installs a tested Python host integration that
   depends on the one generic `atm-graft` implementation.
2. Matching wheels install and pass the package/import/bridge gates on both
   Hermes/M4 CPython 3.14 and Hermes/M5 CPython 3.11.
3. Two configured profiles with distinct chat ids prove strict isolation from
   platform chat id to opaque runtime session id.
4. A real incoming nudge from a durable ATM write appears in the intended
   recipient Telegram session at a safe tool boundary, with no normal-message
   injection, interruption, mailbox mutation, or duplicate steer.
5. The one post-listening recovery summary behaves exactly as ADR-043 defines.
6. The live report, package test, focused unit tests, CI, and quality review
   are all linked from the AL.16 PR.
