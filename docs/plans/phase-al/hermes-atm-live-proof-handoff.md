# Hermes ATM Live-Proof Handoff

**applies to:** AL.16 first gate, then AL.17/AL.18

**required readers:** Cipher-311d and `skillrx@hermes`

**source of truth:** this handoff, [AL.16](sprint-AL16-hermes-graft-live-proof.md),
and [ADR-043](../../adr/ADR-043-hermes-graft-wake-up-ownership.md). If an
older prototype, memory, or bridge document conflicts, this handoff and
ADR-043 win.

## Goal and first gate

Prove one real, durable ATM write reaches the intended existing Telegram
session as an **inbound, host-originated ATM nudge**, emits a visible notice,
and starts the normal Hermes agent turn and response in that same chat.

This is a stop/go gate. Do not broaden package extraction, profile recovery,
multi-profile work, interpreter coverage, or PyPI publication until this
single installed-package proof passes or yields a concrete, reproducible
defect.

## Exact consumption contract

The phrase “inbound event” has a precise meaning here. It is a typed
ATM-originated host signal delivered through the configured profile's real
Telegram adapter. It is **not** a separate `Platform.ATM` session and not a
fake Telegram update from the network:

```text
separate ATM sender durable write
  -> one capability-authenticated graft receiver
  -> installed generic atm-graft wheel
  -> installed hermes-atm runtime in the active Hermes profile
  -> typed PyNudge callback on the gateway event loop
  -> deployed GatewayRunner capability selects the configured existing
     Telegram session from explicit profile + ATM_CHAT_ID
  -> one visible host-originated Telegram notice
  -> internal event for that existing Telegram session
  -> normal model turn and normal Telegram response
```

This preserves the intended “incoming nudge for SkillRX” behavior while
protecting user-session semantics:

- The ATM nudge is input to the correct SkillRX profile's existing Telegram
  session and starts the normal model turn when idle. If the session is busy,
  its internal event queues silently behind the active turn; it must not
  inherit normal Telegram input's disruptive `interrupt` default or call
  `steer`. Human Telegram input may still use the existing `/queue` and
  `/steer` controls.
- It must never impersonate a remote Telegram user or create a separate ATM
  platform/session. The internal event is intentionally local to the selected
  real Telegram adapter.
- The visible Telegram notice identifies that the host nudge arrived. It must
  not expose raw private message contents by default.
- Mail remains daemon-owned. Nudge receipt must not automatically read,
  acknowledge, mutate, retry, replay, or persist mail. The normal nudge body
  is `read atm`.

## Package and repository boundary

| Area | Owner | Required boundary |
| --- | --- | --- |
| Generic receiver/client | Cipher in `atm-core` | `atm-graft` remains a generic installed wheel: no Hermes/Telegram imports, direct storage, second client, replay, or host policy. |
| Hermes composition | Cipher with SkillRX | `hermes-atm` is the separately installed integration package. It owns explicit profile configuration, event-loop handoff, and the documented runner call. It never selects an adapter or constructs a session itself. |
| Live gateway seam | SkillRX in Hermes Agent | Hermes Agent owns the existing Telegram adapter/session, gateway lifecycle, visible notice, and queue behavior through the deployed runner capability. Hermes source changes are reviewed in Hermes Agent, not copied into `atm-core`. |
| Live profile/harness | SkillRX | The `.hermes` profile receives a built wheel and declarative configuration only. Never import an ATM checkout with `PYTHONPATH`/`sys.path` or edit generated receiver JSON. |
| Architecture/review | ATM integration owner | Decide boundary questions, review evidence, and direct a smallest isolated fix to the correct repository. |

The Python package names are `atm-graft` (generic) and `hermes-atm` (Hermes
integration, import name `hermes_atm`). Python package versions are final PEP
440 `1.4.x` values; they never inherit the daemon's `-beta-ai-N` tag.

## Current prerequisite: publish a real receiver endpoint

The current live SkillRX receiver record is stale schema v1. It is not a
valid AL.16 endpoint and must not be hand-edited, accepted as a fallback, or
converted in place.

SkillRX must use the real profile's supported activation/restart path to start
the current installed receiver and publish a fresh schema-v2, generation-owned
endpoint at the roster-resolved graft root. If profile and workspace roots need
an approved linkage, configure that linkage before activation; the running
receiver—not an operator—creates the record and capability.

Only after direct ATM delivery to SkillRX succeeds may the live-session proof
start. If direct delivery still fails, report the exact reader/publisher root,
schema observed, process ownership state, and failure boundary. Do not work
around it by editing endpoint JSON or changing recipient identity data.

## Responsibilities and execution order

1. **SkillRX — restore publication.** Restart/re-activate the actual live
   Hermes profile using the installed package. Confirm a current-schema,
   generation-owned receiver is listening. Send Cipher a concise status with
   no raw capability, chat id, or private path.
2. **Cipher — prove direct transport.** Send one bounded test nudge to the
   restored endpoint and confirm delivery succeeds. If it fails, capture the
   exact ATM/graft boundary error and stop; do not modify the Hermes harness.
3. **SkillRX + Cipher — run the first gate.** Send one unique durable marker
   while the gateway is already running. The installed `hermes-atm` runtime
   must use the typed callback to select the configured real Telegram adapter,
   send its notice, and inject the internal event without restart.
4. **SkillRX — prove host handling.** Demonstrate the marker reaches
   `agent:main:telegram:dm:<ATM_CHAT_ID>`, a host-originated notice appears in
   the intended Telegram chat, and the ordinary agent response follows. Verify
   no separate ATM session, network-synthetic Telegram update, interrupt,
   automatic read/ack, or duplicate delivery occurred.
5. **Cipher — collect the ATM side.** Retain redacted durable-write and
   receiver-delivery evidence. Verify no automatic read/ack, retry, replay, or
   duplicate callback delivery occurred.
6. **Both — report only decisive evidence.** Send the ATM integration owner a
   concise completion summary or one exact missing seam. Do not flood its
   mailbox with individual test markers or acknowledgements.

## Required evidence

The completion report must link or retain, without secrets or raw chat ids:

- installed wheel names, final PEP 440 versions, interpreter version, and
  source commits for `atm-core` and Hermes Agent;
- healthy ATM daemon/receiver readiness and a redacted current-schema endpoint
  generation;
- durable marker created by a separate sender and accepted receiver delivery;
- selected Telegram session-key evidence and callback/injection acceptance;
- the host-originated Telegram notice and ensuing normal agent output; and
- negative evidence: no separate ATM session, no network-synthetic Telegram
  update, no automatic read/ack, no durable graft state, no retry/replay, and
  no second receiver or duplicate delivery.

## Stop conditions and escalation

Stop immediately and report the exact seam if any of these occurs:

- endpoint publication is stale, malformed, or rooted where daemon lookup
  cannot resolve it;
- the runtime imports a checkout or old standalone prototype rather than the
  installed wheels;
- the only available Hermes path creates a separate ATM session;
- the selected session key is not the existing Telegram session;
- a nudge causes a restart, read/ack/replay/retry, or exposes message text in
  the notice.

SkillRX may fix a Hermes Agent or profile-lifecycle defect in the Hermes
repository; Cipher may fix the smallest ATM package defect in a fresh
`integrate/phase-al` worktree. Each code fix gets its own reviewed PR. Neither
agent should patch the other repository or modify generated runtime state.
