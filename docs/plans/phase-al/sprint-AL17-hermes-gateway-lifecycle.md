# AL.17 — Released Hermes Host Contract for `hermes-atm`

**implementation repository:** Hermes Agent, then deployed into the live `.hermes` harness
**ATM dependency:** AL.16's separately built `atm-graft` and `hermes-atm` candidate wheels
**owners:** Hermes Agent maintainers (public host API), `skillrx@hermes` (profile operator and deployment evidence), Cipher-311d (ATM package coordination), ATM integration owner (boundary review)
**goal:** make the Hermes capability consumed by `hermes-atm` a released, versioned, documented host contract rather than an M4-local source edit.

## Why this sprint exists

The package-side MVP is deliberately small: a typed graft `PyNudge` calls the host runner with the configured Hermes profile and Telegram chat identity. The runner owns the real adapter, session key, visible notice, and agent-loop dispatch. That is the only safe way to keep ATM from manufacturing a Telegram session or coupling itself to private adapter internals.

M4 has demonstrated an implementation of `GatewayRunner.inject_internal_message(...)`, but the implementation and its test presently exist only in a dirty local Hermes checkout. M5's released Hermes Agent 0.19 has no such public capability. A package that relies on the local edit may be useful exploratory evidence; it is not deployable software. AL.17 closes that portability gap.

## MVP delivery semantics

The supported MVP mode is **queue**. An ATM nudge is a local, host-originated event for the configured agent's existing Telegram session:

```text
durable ATM write
  -> recipient graft receiver
  -> installed hermes-atm callback
  -> released GatewayRunner.inject_internal_message(...)
  -> one visible 📬 notice in the configured Telegram chat
  -> internal event in that exact existing Telegram session
  -> normal Hermes message pipeline
```

When the session is idle, the internal event may start the normal agent loop. When it is busy, Hermes queues the event behind that same session's active turn and drains it exactly once after completion. It must not interrupt the turn, impersonate a Telegram network user, create a second ATM session, or read/ack/replay mail.

`steer` is **not** part of this MVP. Queue is the first proven seam, not a permanent product preference. ATM queue and steer modes will be designed as separate, explicitly selected capabilities in a later sprint; no AL.17 code may silently call steer or encode a final policy decision.

## Required public Hermes contract

Hermes Agent must publish, test, document, and version a profile-aware runner capability equivalent to:

```python
await runner.inject_internal_message(
    *, profile: str, platform: Platform, chat_id: str,
    text: str, notice_text: str | None = None,
)
```

The exact spelling may differ only if the Hermes Agent documentation and the `hermes-atm` adapter are updated together. The contract must specify:

1. **Ownership.** The runner, not `hermes-atm`, resolves the adapter and constructs the existing session identity.
2. **Identity.** `profile`, `platform`, and `chat_id` select the target existing session. A source ATM identity or raw source chat id cannot select another session.
3. **Visible notice.** When `notice_text` is supplied, the configured adapter emits one concise user-facing host notice before the internal event. Default ATM notices must not expose the durable message body.
4. **Queue semantics.** A busy matching session queues once; it does not interrupt or invoke steer. An idle matching session follows normal runner dispatch. Duplicate/failed delivery behavior is observable and fails closed.
5. **Lifecycle.** The capability is available from a documented plugin or profile-start lifecycle context on the runner's event loop. It does not require `sys.path`, a private object, monkey-patching, or source checkout imports.
6. **Errors.** Missing profile, unavailable Telegram adapter, bad chat id, or unavailable runner capability raise a structured error; `hermes-atm` must refuse receiver activation rather than fall back to direct `adapter.handle_message`.

## Scope and order

### A. Publish the host API in Hermes Agent

1. Move the existing local runner method, if it is still appropriate, into a clean Hermes Agent change with its implementation and regression tests.
2. Test idle dispatch, busy same-session queueing, profile/chat isolation, notice emission, unavailable adapter/profile failures, and no hidden steer or interrupt call.
3. Document the public plugin/profile lifecycle surface that supplies the runner and the compatibility version containing it.
4. Review and release/install the Hermes Agent change through its own normal process. A local checkout diff or untracked test cannot satisfy this step.

### B. Bind and verify `hermes-atm` against that published API

1. In a fresh ATM worktree, keep `hermes-atm` limited to the installed `atm-graft` API plus the documented runner capability. It receives an explicit profile and `ATM_CHAT_ID`; it must not infer either from ATM identity.
2. Add/retain a package test with a fake runner exposing only the public `inject_internal_message` contract. The test must demonstrate that no adapter lookup, synthetic external Telegram update, session-key construction, or direct adapter `handle_message` dependency remains.
3. Build the two wheels, install them into the actual live gateway Python environment, and restart only through Hermes's supported lifecycle.
4. Record active-service executable, Hermes version, imported module root, public capability probe, installed wheel provenance, and a redacted successful receiver publication.

### C. Deployment gates

M4 and M5 must each perform the active-service capability inventory before live delivery. If the installed Hermes version lacks the contract, record the versioned compatibility blocker and stop that live lane. Do not repair it by copying the M4 method, changing `sys.path`, calling a direct adapter method, or treating an isolated wheel import as live compatibility.

## Boundaries

- `atm-graft` remains generic: no Hermes/Telegram imports, session policy, direct storage/socket access, second receiver, retry, or replay state.
- `hermes-atm` remains composition only: explicit profile/chat configuration, receiver lifecycle binding, event-loop handoff, and the documented runner call. It does not supervise the daemon.
- Hermes Agent owns Telegram adapter selection, session identity, notices, queueing, and eventual steer semantics.
- Frozen legacy `crates/atm-daemon` is out of scope. All ATM runtime evidence uses the Tokio/Axum `atm-http-runtime` daemon.

## Acceptance

1. A released, documented Hermes Agent version exposes the required runner capability from a supported profile/plugin lifecycle context.
2. Hermes-side tests prove profile isolation, visible notice behavior, idle dispatch, busy **queue** behavior, and the absence of interrupt/steer in the MVP path.
3. `hermes-atm` uses only that public contract and fails closed when it is not available; tests prove no direct adapter or checkout-import fallback.
4. The installed, active M4 gateway imports the released Hermes Agent and reviewed ATM wheels, publishes one generation-owned receiver, and passes the capability inventory.
5. M5's active-service inventory is rerun against the released contract. AL.19 remains blocked if its installed Hermes version does not yet provide it; a CPython 3.11 wheel-only result is not a substitute.
6. Hermes Agent and ATM package revisions, interpreter versions, and redacted deployment evidence are linked in the report. Only then may AL.18 claim a portable live proof.
