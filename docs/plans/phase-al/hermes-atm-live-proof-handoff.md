# Hermes ATM Live-Proof Handoff

**applies to:** AL.16 first gate, then AL.17/AL.18

**required readers:** Cipher-311d and `skillrx@hermes`

**source of truth:** this handoff, [AL.16](sprint-AL16-hermes-graft-live-proof.md),
and [ADR-043](../../adr/ADR-043-hermes-graft-wake-up-ownership.md). If an
older prototype, memory, or bridge document conflicts, this handoff and
ADR-043 win.

## Goal and first gate

Prove one real, durable ATM write reaches the intended live Hermes session as
an **inbound, host-originated ATM nudge event**, is processed at Hermes's safe
steer boundary, and produces both the ensuing agent output and a visible
host-originated notice in the corresponding Telegram chat.

This is a stop/go gate. Do not broaden package extraction, profile recovery,
multi-profile work, interpreter coverage, or PyPI publication until this
single installed-package proof passes or yields a concrete, reproducible
defect.

## Exact consumption contract

The phrase “inbound event” has a precise meaning here. It does **not** mean a
normal Telegram user message or a synthetic `MessageEvent`. It means a typed
ATM-originated host signal that enters the configured Hermes profile with ATM
provenance and is consumed through Hermes's authenticated, non-interrupting
steer seam:

```text
separate ATM sender durable write
  -> one capability-authenticated graft receiver
  -> installed generic atm-graft wheel
  -> installed hermes-atm runtime in the active Hermes profile
  -> inbound host-originated ATM nudge event on the gateway event loop
  -> resolve configured ATM_CHAT_ID to an opaque live runtime session id
  -> authenticated non-interrupting session.steer
  -> next safe boundary of the current controlled agent run
  -> normal next model output and one visible host-originated Telegram notice
```

This preserves the intended “incoming nudge for SkillRX” behavior while
protecting user-session semantics:

- The ATM nudge is input to the correct SkillRX profile and may appear in that
  profile's model context at the safe boundary.
- It must never impersonate a Telegram user, forge a Telegram `MessageEvent`,
  interrupt a running tool call, or create a second user turn.
- The visible Telegram notice identifies that the host nudge arrived. It is an
  observability item, not a duplicate inbound user message, and must not expose
  raw private message contents by default.
- Mail remains daemon-owned. Nudge receipt must not automatically read,
  acknowledge, mutate, retry, replay, or persist mail.
- AL.16 proves the active-run path only. Idle-session behavior is an explicit
  later decision; it may not silently fall back to normal Telegram ingress.

## Package and repository boundary

| Area | Owner | Required boundary |
| --- | --- | --- |
| Generic receiver/client | Cipher in `atm-core` | `atm-graft` remains a generic installed wheel: no Hermes/Telegram imports, direct storage, second client, replay, or host policy. |
| Hermes composition | Cipher with SkillRX | `hermes-atm` is the separately installed integration package. It owns profile configuration, event-loop handoff, session resolution, and use of Hermes's public steer capability. |
| Live gateway seam | SkillRX in Hermes Agent | Hermes Agent owns the authenticated session registration, safe steer capability, gateway lifecycle, and outbound Telegram notice. Hermes source changes are reviewed in Hermes Agent, not copied into `atm-core`. |
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

Only after direct ATM delivery to SkillRX succeeds may the active-run proof
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
3. **SkillRX + Cipher — run the first gate.** With a controlled SkillRX agent
   run active at a known safe boundary, send one unique durable marker. SkillRX
   verifies the installed `hermes-atm` runtime resolves the configured profile
   to its opaque runtime session id and invokes the authenticated steer seam.
4. **SkillRX — prove host handling.** Demonstrate the marker reaches the same
   active model loop at the next safe boundary and one host-originated notice
   appears in the intended Telegram chat. Demonstrate no normal Telegram user
   handler, interruption, or second turn was invoked.
5. **Cipher — collect the ATM side.** Retain redacted durable-write and
   receiver-delivery evidence. Verify no automatic read/ack, retry, replay, or
   duplicate steer occurred.
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
- resolved opaque runtime session id evidence, accepted steer evidence, and
  proof of safe-boundary insertion into the same active agent run;
- the host-originated Telegram notice and ensuing normal agent output; and
- negative evidence: no `MessageEvent`, no user identity impersonation, no
  automatic read/ack, no durable graft state, no retry/replay, and no second
  receiver or second turn.

## Stop conditions and escalation

Stop immediately and report the exact seam if any of these occurs:

- endpoint publication is stale, malformed, or rooted where daemon lookup
  cannot resolve it;
- the runtime imports a checkout or old standalone prototype rather than the
  installed wheels;
- the only available Hermes path is normal Telegram user-message ingress;
- session resolution uses a raw platform chat id as a runtime session id;
- a nudge causes a read/ack/replay/retry or exposes message text in the notice;
- the live gateway lacks an authenticated steer capability.

SkillRX may fix a Hermes Agent or profile-lifecycle defect in the Hermes
repository; Cipher may fix the smallest ATM package defect in a fresh
`integrate/phase-al` worktree. Each code fix gets its own reviewed PR. Neither
agent should patch the other repository or modify generated runtime state.
