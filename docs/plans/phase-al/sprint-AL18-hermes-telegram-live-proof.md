# AL.18 — Installed `hermes-atm` Queue Live Proof

**execution base:** the deployed Hermes Agent host contract from AL.17 plus reviewed `atm-graft` and `hermes-atm` wheels at frozen revisions
**execution host:** M4/SkillRX's live Telegram profile
**owners:** `skillrx@hermes` (live profile operator), Cipher-311d (ATM package and evidence coordination), Hermes Agent maintainer (host API review), ATM integration owner (acceptance review)
**goal:** prove a real durable ATM write reaches one existing Telegram session through the deployed installed-package **queue** path.

## Preconditions

AL.18 does not begin live traffic until all of these are true:

1. AL.17's public runner capability is deployed from its reviewed immutable
   Hermes commit and present in the actual active gateway—not merely in a local
   Hermes checkout.
2. The active process imports the installed `hermes-atm` and `atm-graft` wheels from their recorded locations, with no `sys.path`, `PYTHONPATH`, or worktree import.
3. The target profile has explicit `ATM_HOME`, `ATM_IDENTITY`, `ATM_TEAM`, and `ATM_CHAT_ID`; the Tokio/Axum daemon and CLI are a matched healthy pair under `atm doctor --json`.
4. The profile runtime publishes one schema-v2, generation-owned graft receiver. Endpoint files are never edited by hand or accepted as a v1 fallback.
5. The report records the exact Hermes Agent release, Python executable and version, both final PEP 440 package versions/wheel tags, and the separate daemon SHA/tag. Neither Python wheel may carry the daemon `-beta-ai-N` suffix.

## Exact contract under proof

```text
separate registered ATM sender durable write
  -> recipient's one graft receiver
  -> typed PyNudge in installed hermes-atm
  -> GatewayRunner.inject_internal_message(
       profile=<configured profile>, platform=TELEGRAM,
       chat_id=<configured ATM_CHAT_ID>, text="read atm",
       notice_text="📬 …")
  -> one visible notice in that existing Telegram chat
  -> Hermes internal event on that exact session
  -> normal agent-loop processing, or one queued later drain
```

The nudge body is `read atm`; the durable message remains in ATM until an ordinary agent action reads and acknowledges it. The nudge itself does not carry message content, invoke a normal external Telegram update, create an ATM platform session, or mutate/replay durable mail.

This sprint proves the ATM package's **queue** mode, not an ATM steer feature. The deployed Hermes host API also exposes an explicit steer capability and AL.17 proves it separately. Here, `hermes-atm` always requests `mode="queue"`: an idle session may process the event through its ordinary runner pipeline, while a busy matching Telegram session queues the internal event and drains it once after the active turn. It must not interrupt the turn or call steer. An ATM feature that selects `mode="steer"` still requires a later design, security review, and separate acceptance plan.

## Required execution order

### 1. Installed-package and readiness evidence

Run the existing managed bridge/install tests; do not create a duplicate smoke runner. Retain redacted output for wheel build/install/import, active-service module provenance, daemon doctor, current receiver generation, and the public runner capability probe. A preflight failure is a stop condition, not a reason to use a private Hermes API.

### 2. Idle existing-session proof

1. From a separate registered sender, send one unique durable marker to the target `skillrx@hermes` recipient while the configured Telegram session is idle.
2. Record durable ATM acceptance, receiver delivery, typed callback, public runner-call acceptance, one visible concise `📬` notice, the selected existing Telegram session, and one normal agent response.
3. Confirm no automatic `atm read`, `atm ack`, retry/replay, second receiver, second session, synthetic external Telegram update, interrupt, or steer occurred.

### 3. Busy same-session queue proof

1. Start a real ordinary Telegram turn in the **same** profile and exact `ATM_CHAT_ID` session. A CLI, cron, different chat, or different profile is not a valid busy baseline because it has a different Hermes session key.
2. While that turn is active, send a second unique durable ATM marker.
3. Prove the event entered that session's queue once, did not interrupt the active turn, did not call steer, and drained exactly once after the first turn finished.
4. Capture timestamps/session identity sufficiently to prove the two events share the same configured Telegram session without publishing raw chat IDs or private message contents.

### 4. Isolation, recovery, and normal mail actions

1. Run an existing two-profile fixture, or live proof if two configured live profiles are available, showing profile A cannot deliver a nudge into profile B.
2. Restart/reconnect through the supported Hermes lifecycle with unread or pending-ack mail. If ADR-043 authorizes a recovery summary, it is one count-only injected notice after listening and never individual-message replay. If that feature is not implemented, record it as out of scope; do not invent it in the live harness.
3. After the nudge, use ordinary ATM `read` then `ack` behavior. The ack must follow the received message's normal reply routing and must not have been performed implicitly by the wake.

## Defect routing and reporting

- An ATM package defect is isolated in a fresh `origin/integrate/phase-al` worktree, tested, and quality-reviewed before rerunning the affected proof row.
- A missing/broken host capability is a Hermes Agent defect, fixed and reviewed in Hermes Agent, then deployed before retrying. It is never worked around by changing the live profile or copying private code into `atm-graft`.
- Commit only redacted evidence through the existing `site/reports` index and navigation. Retain raw diagnostics locally. Do not add a bespoke smoke script or hand-edit endpoint fixtures.

## Acceptance

1. The actual live gateway uses the deployed Hermes artifact recorded by AL.17
   and installed, reviewed ATM wheels; no local checkout dependency remains.
2. The idle durable-write proof produces exactly one visible `📬` notice and one normal result in the intended existing Telegram session.
3. The busy proof uses that same Telegram session and demonstrates exactly one deferred **queue** drain, with no interrupt or steer.
4. Profile/session isolation and all negative properties are proven.
5. Ordinary post-wake read/ack behavior remains explicit and correctly routed.
6. The indexed redacted report links exact artifacts, tests, CI, and the Hermes/ATM quality reviews. A missing deployed host contract, blocked row, or prototype-only result is reported as blocked—not as a live pass.
