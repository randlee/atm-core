# Sprint AQ1.9 — hermes-atm Wheel Bump + Live Verification

Status: draft · Branch: `feature/aq-1-9-hermes-wheel-verify` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Final graft connection-model sprint (see AQ1.5). The Python surface never
touches the record file (verified: `hermes_atm/runtime.py` /
`native_tools.py` only import the `atm_graft` PyO3 binding), so this is a
dependency bump plus live proof that the Hermes workaround era is over —
coordinated with team-lead@atm-dev on M5, framed per standing convention as
an atm-graft API change notification, not a Python code change request.

## Deliverables

1. Rebuild/bump the `atm-graft-python` wheel against the AQ1.5–AQ1.8
   crates; version note in the wheel changelog naming the registration
   cutover and ADR-056.
2. **Live verification on m5** (the Hermes host): a real Hermes agent
   session sends/receives via graft across BOTH restart orders — daemon
   restarted under a live receiver, and receiver restarted under a live
   daemon — with zero manual steps (no profile reset). Transcript +
   `atm doctor --json` graft section captured as sprint evidence.
3. Notify M5 team-lead (ATM message) with the cutover summary and the
   evidence, and collect confirmation that the previously reported
   endpoint-decode failures / CLI-file workarounds are no longer needed.

## Acceptance criteria

1. Wheel builds green against the phase branch; hermes-atm's own test
   suite (as run on M5) passes with the new wheel.
2. The restart-matrix live evidence shows delivery recovering
   automatically in both orders (timestamps + message ids in the
   transcript).
3. M5 team-lead confirmation recorded (message id in the sprint evidence
   notes).

## Non-closure / out of scope

- Any hermes-atm Python source change (none required).
- Hermes-side `/queue` routing (AQ2's Python-surface deliverable).

## Dependencies

- must_follow: AQ1.7 (cutover must be live before verification proves it).
  PR-completion trigger: AQ1.7 PR merges first.
- parallel_safe: AQ1.8 (disjoint files; see AQ1.8).
