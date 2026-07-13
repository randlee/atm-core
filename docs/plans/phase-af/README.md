# Phase AF — 1.3.1 Reliability Recovery

## Decision

Use **Phase AF** for the 1.3.1 recovery. It is split into three production
sprints because the host-wide runtime invariant, observability/release-hardening
work, and the native send-input data path cannot credibly close at production
quality in one sprint. AF-1 is the release blocker; AF-2 and AF-3 may start
only after AF-1 has an accepted design and its process-level proof is green.

| Sprint | Closure | Release gate |
| --- | --- | --- |
| [AF-1: host singleton](af-1-host-singleton.md) | One `atm-daemon` per OS user/host, with no `ATM_HOME`, socket, or test exception bypass. | Required before any further full smoke that can launch a daemon, and before 1.3.1 RC. |
| [AF-2: observability and release gates](af-2-observability-release-gates.md) | Accurate doctor hook disclosure, actionable daemon errors, hermetic validation, and cutover safeguards. | Required before declaring 1.3.1 release-ready. |
| [AF-3: native send-input integrity](af-3-native-send-input-integrity.md) | `stdin`, inline, and file send sources reach the daemon with the intended bytes and typed local failures. | Required before declaring 1.3.1 release-ready. |

## Evidence baseline

The authoritative smoke evidence is `reports/smoke/smoke-thorough.md` and
`release-findings.json` in commit `9e01e19e`:

- `SMOKE-FIND-001` is release-blocking: three daemons ran concurrently when
  launchers used distinct `ATM_HOME` roots.
- `SMOKE-FIND-005` and `SMOKE-FIND-007` are the two user-visible diagnostics
  gaps: healthy doctor output hides retained daemon errors and active nudge
  overrides.
- `SMOKE-FIND-002`, `003`, `004`, and `006` supply the release-process and
  configuration follow-through for AF-2.
- The post-plan native-CLI smoke finding (`atm send <to> --stdin`) is AF-3:
  its bytes are currently sent to the daemon as a `Stdin` marker even though
  the daemon process is intentionally spawned with null stdin.

No Phase AF sprint permits a test-only alternate runtime root, endpoint, lock,
or daemon launch path. Tests must prove the production invariant from an
isolated OS user/CI host, not weaken it in the product.
