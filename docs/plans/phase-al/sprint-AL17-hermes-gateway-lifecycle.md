# AL.17 — Deployable Hermes Host Contract for `hermes-atm`

**implementation repository:** Hermes Agent, then deployed into the live `.hermes` harness
**ATM dependency:** AL.16's separately built `atm-graft` and `hermes-atm` candidate wheels
**owners:** Hermes Agent maintainers (public host API), `skillrx@hermes` (profile operator and deployment evidence), Cipher-311d (ATM package coordination), ATM integration owner (boundary review)
**goal:** make the Hermes capability consumed by `hermes-atm` a reviewed, immutable, documented, deployed host contract rather than an M4-local source edit.

## Why this sprint exists

The package-side MVP is deliberately small: a typed graft `PyNudge` calls the host runner with the configured Hermes profile and Telegram chat identity. The runner owns the real adapter, session key, visible notice, and agent-loop dispatch. That is the only safe way to keep ATM from manufacturing a Telegram session or coupling itself to private adapter internals.

M4 has demonstrated an implementation of `GatewayRunner.inject_internal_message(...)`, but the implementation and its test presently exist only in a dirty local Hermes checkout. M5's installed Hermes Agent 0.19 has no such public capability. A package that relies on the local edit may be useful exploratory evidence; it is not deployable software. AL.17 closes that portability gap.

## Contract artifact rule

Hermes Agent has no formal public release process. Therefore **deployable** in
AL.17–AL.19 has a concrete, auditable meaning: the host API is in a clean,
reviewed Hermes Agent commit with an immutable SHA, and the active gateway
imports an artifact built or installed from that exact commit. The report must
record both the reviewed source SHA and active-process module provenance.

A dirty checkout, untracked test, local monkey-patch, or a method copied into
only one harness is not deployable. This rule deliberately does not require a
PyPI upload or externally versioned Hermes distribution; it requires a
reproducible reviewed deployment that M4 and M5 can identify and install.

## MVP delivery semantics

The supported MVP mode is **queue**. An ATM nudge is a local, host-originated event for the configured agent's existing Telegram session:

```text
durable ATM write
  -> recipient graft receiver
  -> installed hermes-atm callback
  -> deployed GatewayRunner.inject_internal_message(...)
  -> one visible 📬 notice in the configured Telegram chat
  -> internal event in that exact existing Telegram session
  -> normal Hermes message pipeline
```

The host API has two explicit delivery capabilities. `queue` is the default: an idle session follows normal runner dispatch, while a busy matching session queues once and drains after its active turn. `steer` is explicit only: it targets the already-running, exact profile/chat session through Hermes's normal steer seam. It must never be selected implicitly, cross a profile or chat boundary, manufacture a second ATM session, impersonate a Telegram network user, or read/ack/replay mail.

AL.17 proves both host capabilities because they belong in one reviewed Hermes API. The ATM-side MVP remains queue-only: `hermes-atm` and AL.16–AL.19 always request `mode="queue"`. An ATM feature that deliberately selects steer remains a later, separately reviewed product decision.

## Required public Hermes contract

Hermes Agent must publish, test, document, and version a profile-aware runner capability equivalent to:

```python
await runner.inject_internal_message(
    *, profile: str, platform: Platform, chat_id: str, text: str,
    notice_text: str | None = None,
    mode: Literal["queue", "steer"] = "queue",
)
```

The exact spelling may differ only if the Hermes Agent documentation and the `hermes-atm` adapter are updated together. The contract must specify:

1. **Ownership.** The runner, not `hermes-atm`, resolves the adapter and constructs the existing session identity.
2. **Identity.** `profile`, `platform`, and `chat_id` select the target existing session. A source ATM identity or raw source chat id cannot select another session.
3. **Visible notice.** When `notice_text` is supplied, the configured adapter emits one concise user-facing host notice before the internal event. Default ATM notices must not expose the durable message body.
4. **Queue semantics.** `mode="queue"` queues a busy matching session once and drains it after the active turn; it does not interrupt or invoke steer. An idle matching session follows normal runner dispatch.
5. **Steer semantics.** `mode="steer"` is explicit and may call Hermes's steer seam only for the resolved, actively running profile/chat session. Its no-active-turn behavior is documented and tested; it must not silently target another session. Duplicate/failed delivery behavior is observable and fails closed.
6. **Lifecycle.** The capability is available from a documented plugin or profile-start lifecycle context on the runner's event loop. It does not require `sys.path`, a private object, monkey-patching, or source checkout imports.
7. **Errors.** Missing profile, unavailable Telegram adapter, bad chat id, unsupported mode, or unavailable runner capability raise a structured error; `hermes-atm` must refuse receiver activation rather than fall back to direct `adapter.handle_message`.

## Scope and order

### A. Publish the host API in Hermes Agent

1. Move the existing local runner method, if it is still appropriate, into a clean Hermes Agent change with its implementation and regression tests.
2. Test idle dispatch, busy same-session queueing, explicit same-session steer delivery, profile/chat isolation for both modes, notice emission, unavailable adapter/profile failures, and invalid-mode failure. Queue tests must prove no hidden steer or interrupt call.
3. Document the public plugin/profile lifecycle surface that supplies the runner and the compatibility version containing it.
4. Review the Hermes Agent change, record its immutable SHA, and deploy the
   exact resulting artifact through the supported gateway workflow. A local
   checkout diff or untracked test cannot satisfy this step.

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
- Hermes Agent owns Telegram adapter selection, session identity, notices, queueing, and steer semantics. `hermes-atm` has no direct adapter or session-state access.
- Frozen legacy `crates/atm-daemon` is out of scope. All ATM runtime evidence uses the Tokio/Axum `atm-http-runtime` daemon.

## Acceptance

1. A clean, reviewed, immutable, deployed Hermes Agent commit exposes the
   required runner capability from a supported profile/plugin lifecycle context.
2. Hermes-side tests prove profile isolation, visible notice behavior, idle dispatch, busy **queue** behavior without interrupt/steer, and explicit same-session **steer** behavior. The queue default and invalid-mode error are pinned.
3. `hermes-atm` uses only that public contract and fails closed when it is not available; tests prove no direct adapter or checkout-import fallback.
4. The installed, active M4 gateway imports the deployed Hermes artifact and
   reviewed ATM wheels, publishes one generation-owned receiver, and passes the capability inventory.
5. M5's active-service inventory is rerun against the deployed contract. AL.19 remains blocked if its installed Hermes artifact does not yet provide it; a CPython 3.11 wheel-only result is not a substitute.
6. Hermes Agent and ATM package revisions, interpreter versions, and redacted deployment evidence are linked in the report. Only then may AL.18 claim a portable live proof.
7. The installed M4 gateway demonstrates an explicitly selected `mode="steer"` injection into one active, configured Telegram session without crossing profile/chat boundaries. This is host-capability evidence only; it does not authorize an ATM steer mode.
