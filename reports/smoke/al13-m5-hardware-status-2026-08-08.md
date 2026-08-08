# AL.13 M5 hardware-smoke status

Candidate: `cbffedec` (`feature/al-13-smoke`)

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
| Direct M5↔M4 send/read | `just smoke crosshost-send rand-m4.local` | BLOCKED | Blocked by required peer-preflight failure. |
| Cross-host acknowledgement | `just smoke crosshost-ack rand-m4.local` | BLOCKED | Blocked by required peer-preflight failure. |
| Benchmark/report | `just benchmark`; `just benchmark-report` | BLOCKED | AL.13 requires stopping at the first failing row. |

## Blocker

The original missing peer identity/team configuration is resolved by the
isolated `m5-test@atm-dev` mailbox. M4's noninteractive PATH is resolved by
setting `ATM_SMOKE_REMOTE_ATM=/opt/homebrew/bin/atm`. The remaining blocker is
local to M5: after a healthy managed-pair restart, the daemon can disappear
without a graceful-shutdown record, leaving stale IPC state that causes
launchd restarts to fail with `EX_USAGE`. No further M4 traffic is required to
reproduce this failure; the peer-preflight, cross-host, and benchmark rows
remain blocked until that daemon lifecycle issue is resolved.
