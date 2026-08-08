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
| Direct M5↔M4 send/read | `just smoke crosshost-send rand-m4.local` | BLOCKED | Blocked by required peer-preflight failure. |
| Cross-host acknowledgement | `just smoke crosshost-ack rand-m4.local` | BLOCKED | Blocked by required peer-preflight failure. |
| Benchmark/report | `just benchmark`; `just benchmark-report` | BLOCKED | AL.13 requires stopping at the first failing row. |

## Blocker

The configured M4 peer identity/team is absent from the smoke environment.
No repository transport setting or host address was changed. Supplying the two
existing-environment values and rerunning from peer preflight is required
before the blocked rows can be attempted.
