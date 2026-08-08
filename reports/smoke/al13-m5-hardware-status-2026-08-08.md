# AL.13 M5 hardware-smoke status

Candidate: `ec77e0eeec733a9c96deecfbc56becd5c29e15a1` (`feature/al-13-smoke`)

Platform/host: macOS / `rand-m5.local`

Selected CLI and daemon: `1.4.1-beta-ai-1` (daemon signature authority:
`atm-daemon-dev`)

| Row | Command | Result | Retained evidence / detail |
| --- | --- | --- | --- |
| Runtime health | `atm doctor --json` | PASS | Selected pair ready before live rows. |
| Localhost, initial | `just smoke localhost` | FAIL | `site/reports/smoke/macos/rand-m5.local/20260808T163039505849Z-pid91333-localhost/`; daemon stopped and the unowned UDS pathname made the LaunchAgent exit 64. |
| Localhost, recovery rerun | `just smoke localhost` | PASS | `site/reports/smoke/macos/rand-m5.local/20260808T163127790186Z-pid92078-localhost/`; all ten attempts completed send/read and both acknowledgement directions. |
| Local IP | `just smoke local-ip` | PASS | `site/reports/smoke/macos/rand-m5.local/20260808T163200324785Z-pid92336-local-ip/`; all ten attempts passed at the advertised address. |
| Peer readiness | `just smoke peer-preflight rand-m4.local` | FAIL | `site/reports/smoke/macos/rand-m5.local/20260808T163245566263Z-pid92656-peer-preflight/`; `ATM_SMOKE_REMOTE_IDENTITY` and `ATM_SMOKE_REMOTE_TEAM` are unset. |
| Peer readiness, isolated M4 identity | `ATM_SMOKE_REMOTE_IDENTITY=m5-test ATM_SMOKE_REMOTE_TEAM=atm-dev just smoke peer-preflight rand-m4.local` | FAIL | `site/reports/smoke/macos/rand-m5.local/20260808T174153465169Z-pid4796-peer-preflight/`; all ten M5 local-IP and loopback send/read/ack cycles passed, but M4's noninteractive environment could not resolve the default `atm` command (`env: atm: No such file or directory`). |
| Peer readiness, explicit M4 CLI path | `ATM_SMOKE_REMOTE_IDENTITY=m5-test ATM_SMOKE_REMOTE_TEAM=atm-dev ATM_SMOKE_REMOTE_ATM=/opt/homebrew/bin/atm just smoke peer-preflight rand-m4.local` | BLOCKED | The run never contacted M4: after a healthy managed-pair restart, the M5 daemon disappeared without a graceful-shutdown log record and launchd entered an `EX_USAGE` stale-UDS/owner-lock restart loop. The smoke runner was stopped after three local `doctor` timeouts. |
| Peer readiness, one-attempt reproduction | `ATM_SMOKE_REPETITIONS=1` with the same isolated M4 configuration | FAIL | `site/reports/smoke/macos/rand-m5.local/20260808T174845662386Z-pid7510-peer-preflight/`; immediately after a 12-second idle health probe passed, the first smoke-runner local `atm doctor` timed out. No peer command or M4 mailbox access occurred. |
| Peer readiness, signed-pair rerun | `ATM_SMOKE_REMOTE_IDENTITY=m5-test ATM_SMOKE_REMOTE_TEAM=atm-dev ATM_SMOKE_REMOTE_ATM=/opt/homebrew/bin/atm just smoke peer-preflight rand-m4.local` | FAIL | `site/reports/smoke/macos/rand-m5.local/20260808T185302419778Z-pid17915-peer-preflight/`; signing removed the M5 doctor timeouts and all ten M5 local rows passed. M4 preflight then correctly rejected the incompatible remote CLI/daemon version `1.4.1-beta-ai-15` (candidate requires `1.4.1-beta-ai-1`). |
| Candidate local regression guard | `just smoke localhost`; `just smoke local-ip` | PASS | `site/reports/smoke/macos/rand-m5.local/20260808T191357786414Z-pid23030-localhost/` and `site/reports/smoke/macos/rand-m5.local/20260808T191410895708Z-pid23245-local-ip/`; each completed all ten retained repetitions at `1.4.1-beta-ai-1`. |
| Candidate peer readiness | `ATM_SMOKE_REMOTE_IDENTITY=m5-test ATM_SMOKE_REMOTE_TEAM=atm-dev ATM_SMOKE_REMOTE_ATM=/opt/homebrew/bin/atm just smoke peer-preflight rand-m4.local` | PASS | `site/reports/smoke/macos/rand-m5.local/20260808T191457421442Z-pid23503-peer-preflight/`; all repetitions show ready, version-matched M4 and M5 pairs. |
| Direct M5↔M4 send/read | Same isolated `m5-test` configuration with `just smoke crosshost-send rand-m4.local` | PASS | `site/reports/smoke/macos/rand-m5.local/20260808T191605199584Z-pid24045-crosshost-send/`; all ten repetitions prove exact-body, exact-ID delivery in both directions. |
| Cross-host acknowledgement | Same isolated `m5-test` configuration with `just smoke crosshost-ack rand-m4.local` | BLOCKED | Required-message delivery succeeds in both directions, but the acknowledgement reply is not readable on its peer. A subsequent bounded rerun stopped at M4 preflight because M4 `atm doctor --json` exceeded the runner's 20-second timeout; no complete artifact was emitted for that interrupted attempt. |
| Benchmark/report | `just benchmark`; `just benchmark-report` | BLOCKED | AL.13 requires stopping at the first failing row. |

## Blocker

The isolated `m5-test@atm-dev` mailbox and the explicit M4 CLI path are now
valid, and both hosts selected the frozen beta-ai-1 candidate. G3 through G5
therefore pass with retained evidence.

G6 exposes a product routing gap. A host-qualified `atm send` selects the
direct peer client in `crates/atm/src/composition.rs`, but ordinary `atm ack`
is submitted to the local daemon. The acknowledgement code preserves the
peer reply target and durable reply record, yet the current post-write path
does not dispatch that host-qualified reply to its peer. Consequently both
directions prove delivery of the required message but fail the required
readback of the reply ULID and `acknowledgesMessageId`. This is not presented
as a smoke success. Later bounded retry was also blocked when M4's remote
doctor call exceeded the 20-second preflight timeout. G7 remains prohibited
until G6 is fixed and a complete ten-repetition artifact passes.
