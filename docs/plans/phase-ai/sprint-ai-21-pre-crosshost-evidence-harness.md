---
title: AI.21-pre cross-host evidence harness
status: proposed
branch: feature/pAI-s21pre-crosshost-evidence-harness
target: integrate/phase-AI
base: integrate/phase-AI
depends_on: AI.21
blocks: AI.22, AI.25, AI.26, AI.27, AI.28, AI.29
---

# AI.21-pre — cross-host evidence harness

## Release candidate

- First commit: set every releasable ATM assembly to
  `1.3.2-beta-21-pre`; record matching CLI/daemon values from
  `atm doctor --json` in every smoke pane.

## Closure

The mixed `evidence/phase-ai-crosshost-smoke` investigation branch (commit
`3f08041e18cb32dee34e7555bd2cc2c4b51ca938`) is reduced to one small,
reviewable implementation branch: a supported Python/sc-compose smoke harness
and an explicit non-durable plaintext peer-wire profile for diagnosis. QA can
run this branch's release daemon on real hosts before any AI.22+ behavior
change. Nothing else from the evidence branch is adopted.

## Deliverables

1. Add `docs/plans/phase-ai/evidence-branch-disposition.md`. It must classify
   every evidence-derived candidate as **adopt in a named sprint**,
   **diagnostic artifact only**, or **discard**. The evidence branch is never a
   merge source; adoption is by a normal implementation change on this branch.
2. Adopt, test, and document exactly these existing evidence assets as the one
   supported smoke surface:

   ```text
   scripts/smoke/run_inbound_peer_smoke.py
   scripts/smoke/combine_inbound_peer_smoke.py
   scripts/smoke/analyze_logs.py
   scripts/smoke/inbound-peer-smoke.example.json
   templates/smoke-report/inbound-peer-pane.xhtml.j2
   templates/smoke-report/inbound-peer-review.xhtml.j2
   ```

   `run_inbound_peer_smoke.py` receives a declarative JSON configuration; it
   never starts, stops, switches, or configures a daemon. It checks the
   already-running branch daemon with `atm doctor --json`, runs declared cases,
   captures bounded sanitized logs, writes canonical JSON, and renders exactly
   one host XHTML pane through `sc-compose`. `combine_inbound_peer_smoke.py`
   fails for a missing, stale, malformed, or wrong-host pane and renders one
   combined XHTML review page through `sc-compose`; no shell-only alternate
   runner or hand-written report is permitted.
3. The initial required table rows are `doctor`, localhost peer send/read/nudge,
   advertised/self-IP peer send/read/nudge, and inbound remote no-ack plus
   requires-ack/read/nudge. A result row is PASS, FAIL, or NOT-RUN—never an
   inferred success. Every table row records the expected sprint daemon
   version and the actual running daemon version; actual below expected is a
   hard FAIL, never NOT-RUN or inferred PASS. Each pane includes exact commit,
   CLI/daemon/schema/API versions, daemon PID/listener identity, bounded
   session logs, and a concise generated assessment naming every failed row
   and the next investigation target. A required failed row makes the runner
   exit nonzero.
4. Add the only approved debug wire-security option to
   `crates/atm-daemon/src/main.rs`, `composition.rs`, and
   `https_transport.rs`:

   ```text
   atm-daemon --peer-wire-security mutual-tls       # default
   atm-daemon --peer-wire-security plaintext-test   # explicit smoke only
   ```

   `plaintext-test` is process-local, non-durable, and never selected by an
   environment variable. It disables TLS, certificate pinning, and peer
   allowlist enforcement for that daemon process only. A restart without the
   flag restores mTLS. It must use the exact same HTTP resource,
   `WriteRequest`, `ApiRouter`, persistence, and post-write path as mTLS;
   it must not create a plaintext-only message shape, router, nudge, ACK, or
   fallback.
5. In `crates/atm-core/src/api.rs`, represent plaintext-test source-host data
   as separately typed **untrusted smoke provenance**. It cannot construct the
   normal authenticated-peer context, authorize a recipient, or be described
   as peer authentication. The test-only adapter may add the provenance header
   only for canonical writes; read-only HTTP endpoints remain curl-accessible
   without it.
6. Expose active wire security in doctor, retained logs, runner JSON, and XHTML
   as `mutual_tls` or `plaintext_test`. The plaintext profile must be visibly
   non-production and must never satisfy mTLS/allowlist evidence.

## Acceptance criteria

- A release-built `1.3.2-beta-21-pre` daemon is running as exactly one managed
  process on every participating host; each pane proves its matching CLI,
  daemon, listener, and doctor readiness.
- The Python runner produces deterministic JSON and a valid XHTML pane for
  every host; the combiner rejects stale/missing/mislabelled panes and emits a
  single current XHTML page with one pane per host.
- Every initial table row reports a literal PASS/FAIL/NOT-RUN result and
  contains bounded logs plus an assessment. Raw TCP reachability and local
  sender persistence alone cannot produce PASS for an inbound remote row.
- Default startup is mTLS. A TLS, pin, or allowlist failure never falls back to
  plaintext. `plaintext-test` is enabled only by the explicit daemon argument,
  is shown in doctor/logs/reports, and disappears after a normal restart.
- Plaintext-test and mTLS submit an identical `WriteRequest` to the same
  `ApiRouter`/dispatcher/persistence/post-write seam. Tests prove no alternate
  write, ACK, or nudge path exists.
- Plaintext-test provenance cannot authorize a recipient or pass as an
  authenticated peer. Curl may prove read-only HTTP reachability; it cannot
  forge production peer authentication.

## Required validation

1. Unit tests for CLI parsing/default, no TLS-fallback, normal-restart recovery,
   untrusted provenance, router identity, runner JSON validation, XHTML
   escaping, stale/missing pane rejection, and bounded log redaction.
2. `just lint` and `just test` at the exact release-built commit.
3. `daemon-switch.py` and its `SKILL.md` already exist on `develop`.
   AI.29 forward-merges that existing skill into the Phase AI branch line; it
   does not author a new tool. After that merge, arch-ctm runs the skill once
   per sprint on every participating host to set the branch CLI/daemon pair
   and leaves that daemon running. QA and the Python host runner only check
   the already-running daemon through `atm doctor --json`; neither invokes
   daemon-switch. The host runner then runs in mTLS and plaintext-test modes.
   Combine the panes with `sc-compose`; retain the combined XHTML and
   sanitized JSON/log artifacts.
4. QA runs the same release daemon and runner, not a test fixture or the old
   evidence branch daemon. At handoff the branch daemon remains running for
   QA; restore the installed pair only after QA records final completion.

## Non-closure

This sprint does not claim cross-host delivery is reliable, does not add DNS
authority, deadline changes, link-quality state, recovery/backoff, or a remote
delivery queue. It establishes the repeatable test/report surface used to prove
those later changes. Plaintext-test is diagnostic only and does not close
production TLS or allowlist acceptance.
