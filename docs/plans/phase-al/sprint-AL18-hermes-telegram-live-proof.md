# AL.18 — Hermes Telegram Live Proof

**execution base:** the reviewed AL.16 package and AL.17 gateway binding at
one frozen ATM runtime SHA
**owners:** `skillrx@hermes` (live profile execution), Cipher-311d (ATM
coordination/evidence), ATM integration owner (review)
**goal:** prove that a durable ATM write wakes exactly the intended Telegram
session at a safe tool boundary.

## Release boundary for live proof

AL.18 accepts a result only from the two-package installation described in
AL.16: a generic `atm-graft` wheel and a separately versioned `hermes-atm`
wheel. Cipher and SkillRX may continue fixing `hermes-atm` and Hermes Agent in
the harness until this proof passes. They must not make a live result depend on
a local checkout import or on Hermes/Telegram behavior added to `atm-graft`.
If a generic adapter change is genuinely required, it is an independently
reviewed `atm-graft` release; rerun the affected matrix lane with the new
wheel before claiming proof.

The `hermes-atm` wheel under proof is built from its reviewed `atm-core`
commit. A successful harness-only candidate is evidence for the next ATM PR,
not a publishable artifact.

Record the daemon candidate SHA/tag separately from installed Python package
versions. The proof rejects a wheel whose PEP 440 metadata embeds the daemon
`-beta-ai-N` tag; `atm-graft` must be a final 1.4.x release and `hermes-atm`
must resolve its declared final `atm-graft >=1.4,<1.5` dependency.

## Required evidence order

1. **Package matrix.** Retain isolated wheel/install/import evidence for:
   - M4 live Hermes gateway: CPython 3.13;
   - M4 CPython 3.14 compatibility;
   - M5 default Hermes target: CPython 3.11.
   A wheel for one interpreter minor version is not evidence for another.
2. **Reference guards.** On each applicable lane, run the existing bridge and
   safe-steer fixture gates. Do not create a duplicate smoke runner.
3. **Live runtime readiness.** On the M4 live profile, record the frozen
   package versions and ATM SHA, `atm doctor --json`, a listening graft
   receiver, and a redacted current-schema endpoint generation. Never record
   raw chat IDs, credentials, or personal paths in the repository report.
4. **Durable live nudge.** A separate registered ATM sender writes one exact
   marker to `skillrx@hermes`. Retain its durable message id/read result and
   the accepted `session.steer` result. The marker must appear only in the
   configured Telegram session after its next safe tool boundary.
5. **Negative proof.** For that nudge, prove that the resolved opaque runtime
   session ID—not raw configured or source chat id—was used; no normal inbound
   message handler or interruption ran; no mailbox read/ack/mutation occurred
   because of the wake; and a duplicate message id produces no second steer.
6. **Recovery.** Restart/reconnect the profile with durable unread or
   pending-ack work. Exactly ten seconds after listening, observe one concise
   count-only steer; prove no individual-message replay or second summary.
7. **Ordinary ATM actions.** After the wake, use graft’s standard `read` and
   `ack` paths. The acknowledgement must follow normal ATM reply routing;
   waking must not implicitly acknowledge anything.

## Reporting and defect handling

Use the existing bridge/steer smoke entry points and report navigation. Add a
redacted live-proof result to `site/reports`; do not add a bespoke runner or a
hand-edited endpoint fixture. A reproducible package/ATM defect is fixed from
a fresh `origin/integrate/phase-al` worktree with tests and its own reviewed
PR. A Hermes defect is fixed in Hermes Agent with tests and review. A
multi-gateway/multi-chat fan-out policy, retry/replay behavior, or Telegram
security change requires Rand’s explicit decision and a follow-up ADR.

## Acceptance

1. All package lanes are independently reproducible.
2. The live durable-write-to-safe-boundary path passes for SkillRX’s Telegram
   profile, with the stated negative properties.
3. A second profile proves chat/session isolation.
4. One recovery summary conforms exactly to ADR-043.
5. The redacted report, both project PRs, CI, and quality review are linked;
   no blocked or prototype-only result is described as a live pass.
6. The report records separately versioned installed artifacts for both Python
   packages and rejects any result that bypasses the package boundary.
7. The report records final PEP 440 Python versions separately from the daemon
   candidate tag and proves no beta-tag leakage into either installed wheel.
