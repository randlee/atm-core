# AL.16 — Installable `hermes-atm` Package

**branch:** `plan/al16-hermes-graft-live-proof` (this planning branch only)
**implementation base:** a fresh `/sc-git-worktree` from the accepted
`origin/integrate/phase-al` SHA
**owners:** ATM integration owner (architecture/review), Cipher-311d (Python
binding and Hermes coordination)
**goal:** deliver the installable package boundary required before Hermes
gateway wiring and live Telegram proof.

## The MVP model

AL.16 has exactly two installable Python distributions:

1. `atm-graft` is the generic Python adapter around the Rust/PyO3
   `atm-graft` client and receiver capability. It has no Hermes, Telegram,
   gateway, or host-session policy.
2. `hermes-atm` is the selected Hermes-facing Python package. It depends on a
   compatible `atm-graft` wheel and contains only Hermes lifecycle, session
   binding, and steer composition. It can be updated when Hermes Agent changes
   without changing the generic adapter.

A PyPI JSON lookup on 2026-08-09 returned `404` for both `hermes-atm` and the
fallback `atm-hermes`; claim `hermes-atm` at the first authorized package
publication. If that name is claimed before publication, use the fallback
`atm-hermes` and change the package/import documentation in the same PR. This
keeps a Hermes installation explicit:

```text
pip install hermes-atm
```

The current checked-in Maturin project is named `atm-graft` but also ships
Hermes reference modules. AL.16 moves that Hermes-only code into
`hermes_atm`; `atm-graft` retains only generic typed bindings. This fixes the
packaging and ownership boundary without changing ATM transport, storage, the
legacy daemon, or the Telegram gateway protocol. Production gateway wiring is
AL.17; live proof is AL.18.

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

| Requirement / decision | AL.16–AL.18 implementation and proof |
| --- | --- |
| `REQ-GRAFT-PYTHON-001`, ADR-039 | `hermes-atm` uses only the existing PyO3 `atm-graft` API. It does not open a socket, access storage, or add a send/read/ack path. |
| `REQ-GRAFT-RUNTIME-002`, ADR-043.1 | One profile starts one generation-owned receiver. The endpoint record belongs to the receiver, never the Telegram gateway port, and restart reclaims only a stale/dead owner. |
| `REQ-GRAFT-NOTIFY-002`, ADR-043.2/6 | Nudge is a bounded wake signal. Failed steer is observable and fails closed; there is no retry, durable graft state, or message replay. |
| `REQ-GRAFT-HERMES-002`, ADR-039, ADR-043.3 | `ATM_CHAT_ID` is required at startup, resolves through the real Hermes registration/rebind map to an opaque runtime session id, and invokes only `session.steer`. Tests prove one profile cannot steer another profile's chat. |
| `REQ-GRAFT-HERMES-003`, ADR-043.4 | After listening, exactly one ten-second count-only recovery summary may steer the configured session. It must not read, acknowledge, mutate, or replay mail. |

## Delivery order

## Scope

1. Keep `atm-graft` as the one generic Maturin/PyO3 adapter: typed client,
   session activation, endpoint ownership, and typed nudge callback only. It
   must contain no Hermes/Telegram imports or host-session policy.
2. Create a dedicated `hermes-atm` Python distribution which depends on a
   compatible `atm-graft` wheel. Move the existing loader, bridge, and adapter
   into that package, with one documented public runtime entry point:
   `HermesGraftRuntime.from_environment(request=..., resolve_session_id=...)`.
   Do not copy the Rust source or create a second ATM transport client. Claim
   the PyPI project name only through the authorized release workflow; a
   successful HTTP `404` lookup is availability evidence, not ownership.
3. Add an isolated-venv wheel-install test: install `hermes-atm`, import the
   runtime, and show missing `ATM_HOME`, `ATM_IDENTITY`, `ATM_TEAM`, or
   `ATM_CHAT_ID` fails before receiver activation. The test must not need a
   Hermes checkout.
4. Publish the actual wheel name/version/install command in its package
   metadata and operator documentation. Do not claim that the current
   `atm-graft` wheel is already named `hermes-atm`.

### Required Python compatibility evidence

The native PyO3 extension is interpreter-specific unless its build metadata
explicitly proves an `abi3` wheel. AL.16 must therefore build and install—not
merely import from a source checkout—the matching wheel in both supported
Hermes environments:

| Lane | Interpreter | Purpose |
| --- | --- | --- |
| Hermes/M4 live gateway | CPython 3.13 | The currently running Hermes/SkillRX gateway interpreter; live Telegram proof must use this lane. |
| Hermes/M4 compatibility | CPython 3.14 | Explicit upgraded-interpreter wheel compatibility evidence. It becomes a live lane only after Hermes is actually switched to it. |
| Hermes/M5 | CPython 3.11 | Default Hermes-agent compatibility target. |

For each lane, retain the interpreter path/version, wheel filename/tag,
`pip install hermes-atm` result, `just test-hermes-graft-bridge` result, and
the isolated-import/startup-validation result. A wheel built for one CPython
minor version is never accepted as evidence for another. If Maturin/PyO3
cannot build a required lane, treat it as a packaging defect and fix the
supported build configuration; do not silently change the running Hermes
interpreter or bypass pip with `PYTHONPATH`.

## Acceptance

AL.16 is ready to merge only when:

1. `pip install hermes-atm` installs a tested Python host integration that
   depends on the one generic `atm-graft` implementation.
2. The native `atm-graft` wheel and the pure-Python `hermes-atm` wheel are
   separately built, versioned, and have no source-worktree import path.
3. All three interpreter lanes pass the package/import/bridge gates.
4. The generic `atm-graft` wheel contains no Hermes gateway lifecycle,
   Telegram routing, or session-steer implementation.
5. CI and quality review pass. AL.17 cannot begin until its package artifact
   and exact version contract are available.

## Follow-on sprints

- [AL.17 — Hermes Gateway Lifecycle Binding](sprint-AL17-hermes-gateway-lifecycle.md)
  consumes the released/tested package in the actual gateway process.
- [AL.18 — Hermes Telegram Live Proof](sprint-AL18-hermes-telegram-live-proof.md)
  proves durable-write-to-safe-boundary delivery and recovery behavior.
