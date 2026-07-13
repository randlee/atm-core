# Phase AF — 1.3.1 Reliability Recovery

## Decision

Use **Phase AF** for the 1.3.1 recovery. It is split into three production
sprints because the host-wide runtime invariant, observability/release-hardening
work, and the native send-input data path cannot credibly close at production
quality in one sprint. AF-1 is the release blocker; AF-2 and AF-3 may start
only after AF-1 has an accepted design and its process-level proof is green.

| Sprint | Closure | Sprint-local gate |
| --- | --- | --- |
| [AF-1: host singleton](af-1-host-singleton.md) | One `atm-daemon` per OS user/host, with no `ATM_HOME`, socket, or test exception bypass. | Required before any further full smoke that can launch a daemon, and before 1.3.1 RC. |
| [AF-2: observability and release gates](af-2-observability-release-gates.md) | Accurate doctor hook disclosure, actionable daemon errors, hermetic validation, and cutover safeguards. | Required before declaring 1.3.1 release-ready. |
| [AF-3: native send-input integrity](af-3-native-send-input-integrity.md) | `stdin`, inline, and file send sources reach the daemon with the intended bytes and typed local failures. | Required before declaring 1.3.1 release-ready. |

## Shared smoke-script integration contract

`scripts/smoke/run_thorough_shared_host.py` has one cross-sprint owner at a
time. AF1-D5 owns its base structure: the singleton preflight, process-count
capture, and cleanup assertion. AF-1 merges first. AF2-D4 rebases on that AF-1
base and adds installed-artifact selection and release-preflight assertions.
AF3-D3 then rebases on the merged AF-1 and AF-2 script and adds only the
inline/stdin/file durable-body matrix.

Every later change retains the preceding sprint's assertions: AF-2 must retain
AF-1 PID/count and cleanup assertions; AF-3 must retain both AF-1 and AF-2
assertions while adding its input matrix. A merge/rebase that removes or masks
an earlier assertion fails the later sprint's validation. The required merge
order is **AF-1 → AF-2 → AF-3**; parallel work may prepare patches, but its
final merge must rebase in that order.

## Phase release decision criteria

This is the single authoritative Phase AF release checklist.

1. AF-1's process-level singleton suite is green on macOS, Linux, and Windows.
2. AF2-D1 through AF2-D5 validations are green using the release artifacts,
   not `cargo run` or an arbitrary PATH binary.
3. AF-3's release-binary inline/stdin/file input matrix is green against a
   daemon with null stdin.
4. A fresh user-state database can create the team and roster through native
   1.3.1 commands, send/read/ack a message, and show a healthy doctor with no
   unexpected retained errors.
5. The release report lists exact binary versions, PID/count evidence, hook
   selection, doctor status, and any non-empty error snapshot. Any unexpected
   error record is a release blocker until classified and waived explicitly.

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
