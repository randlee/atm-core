---
title: AI.29 receiver-proven Mac-Windows cross-host smoke
status: proposed
branch: feature/pAI-s29-crosshost-smoke-rerun
target: integrate/phase-AI
depends_on: AI.21-pre, AI.24, AI.25, AI.26, AI.27, AI.28, AI.30
---

# AI.29 — receiver-proven Mac↔Windows smoke

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta-29`; record
  matching client/daemon values from `atm doctor --json` in runtime evidence.

## Closure

Mac↔Windows evidence proves each cross-host operation at both sender and
receiver, using the same immutable ULID. Prior sender-only results are
historical diagnostics, not passing evidence.

## Deliverables

1. Extend the AI.21-pre supported Python smoke assets—
   `scripts/smoke/run_inbound_peer_smoke.py`,
   `scripts/smoke/combine_inbound_peer_smoke.py`,
   `scripts/smoke/analyze_logs.py`, and the `templates/smoke-report/*.xhtml.j2`
   sc-compose templates. Do not create a second shell runner or hand-written
   HTML report. Those assets originate from
   `evidence/phase-ai-crosshost-smoke@3f08041e18cb32dee34e7555bd2cc2c4b51ca938`;
   AI.21-pre adopts them by normal implementation change rather than merging
   that investigation branch.
2. Forward-merge the existing `develop` daemon-switch skill —
   `.claude/skills/daemon-switch/SKILL.md` and
   `.claude/skills/daemon-switch/scripts/daemon-switch.py` — into the Phase
   AI branch line. This is branch hygiene, not net-new tool authorship. The
   existing tool queries the active CLI/daemon pair, switches both to a branch
   release pair, and restores the platform's current installed pair without a
   hard-coded path. The runner invokes it only as documented setup; it remains
   lifecycle-free.
3. Require the Python runner on each participating host to produce the
   AI.21-pre-format sanitized
   JSON result and one XHTML pane. Each pane contains: exact commit and
   CLI/daemon/schema/API versions; doctor/liveness/listener identity; a
   PASS/FAIL/NOT-RUN table whose every row records expected sprint daemon
   version and actual running daemon version; bounded structured logs from the
   session; and a generated concise assessment naming every failed row and its
   investigation target. Actual daemon version below expected is a hard FAIL,
   never NOT-RUN or inferred PASS. The runner returns nonzero if any required
   row fails.
4. Require the AI.21-pre `combine_inbound_peer_smoke.py` to use `sc-compose render` to form
   one XHTML review page with one current pane for each required host. It must
   fail for a missing, stale, malformed, or wrong-host pane; it must not infer
   success from raw TCP connectivity or a sender-only CLI result.
5. The runner's required rows are: `doctor`; localhost peer send/read/nudge;
   advertised/self-IP peer send/read/nudge; inbound remote send/read/nudge;
   inbound remote requires-ack/ack/read/nudge; outbound remote send; outbound
   remote requires-ack/ack; duplicate ULID; unavailable peer; wrong
   certificate; and allowlist rejection. Each positive remote row records the
   exact same sender and receiver ULID. Localhost/self-IP rows explicitly
   assert the same ordinary HTTPS endpoint/handler as remote traffic.
6. Run hostname and direct-current-IP cases against one hostname-registered
   peer, including DNS-change stale-IP rejection.
7. Execute the runner against the enabled AI.28 recovery policy: transient
   peer loss, 60-second-minimum reconnect, oldest-first backlog, one active
   host drain, a write arriving during the drain, and a final empty-scan race.
   The report includes `PeerLinkStatus` before and after each failure/recovery.
8. Record one honest unconfirmed-delivery case and verify no false sent claim,
   receipt, retry state, or sender-side ack mutation is created.
9. Capture one-daemon doctor/listener evidence before and after on both hosts;
   restore the normal installed daemon pair after the smoke.

## Implementation map

- Extend only AI.21-pre's `scripts/smoke/run_inbound_peer_smoke.py`,
  `combine_inbound_peer_smoke.py`, `analyze_logs.py`, example JSON, and the
  existing `templates/smoke-report/*.xhtml.j2` templates.
- Arch-ctm runs the daemon-switch skill forward-merged from `develop` once per
  sprint to select the branch CLI/daemon pair and leaves the daemon running.
  QA and the Python runner only check that already-running daemon through
  `atm doctor --json`; neither invokes daemon-switch. Arch-ctm performs the
  mandatory post-smoke restore. The runner itself remains lifecycle-free.
- Add no product transport route, test-only daemon, or shell-report fallback.

## Acceptance criteria

- Every positive case has matching sender request and receiver persisted ULID.
- The existing, forward-merged `daemon-switch.py` queries, switches, and
  restores the CLI/daemon pair on macOS, Linux, and Windows without a
  hard-coded install path.
- The combined XHTML report has one valid, current pane per required host and
  every required table row is PASS; its logs and assessment are sufficient to
  diagnose any failing row without reading unbounded daemon logs.
- Sender local persistence or raw TCP alone fails the evidence validator.
- Negative cases fail before receiver mailbox mutation and expose the expected
  typed error/event.
- Recovery evidence proves only one active socket/drain per host, original
  ULID preservation, oldest-first order, no lost write at final scan, and a
  first reattempt no sooner than 60 seconds.
- Each host records its CLI and daemon release plus negotiated schema and HTTP
  API version; release strings may differ only when the compatibility contract
  succeeds. Both restore their normal pair afterward.

## Required validation

`just lint` and `just test` at the exact tested commit on both hosts; complete
sanitized evidence bundle; runner schema validation; quality review.

## Non-closure

No production feature beyond the AI.25–AI.28 smoke fixes and AI.30
compatibility contract is added here.
