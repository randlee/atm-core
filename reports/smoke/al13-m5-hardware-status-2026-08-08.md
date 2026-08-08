# AL.13 M5 hardware-smoke status

Candidate: `62cb6f1862dced41185ffc87e702d2fc04cf61ca` (`origin/integrate/phase-al`, merged to this evidence branch at `d300e834daa8bd9a0e871f5e18dfc8072c87d3ff`)

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
| ACK routing pre-fix diagnostic | Same isolated `m5-test` configuration with `just smoke crosshost-ack rand-m4.local` | FAIL (superseded) | `site/reports/smoke/macos/rand-m5.local/20260808T193135924612Z-pid27630-crosshost-ack/`; retained on the pre-fix candidate to document the original routing failure and intermittent M4 preflight timeouts. It is not evidence for the selected 62cb6f18 candidate. |
| Cross-host acknowledgement | Same isolated `m5-test` configuration with `just smoke crosshost-ack rand-m4.local` | PASS | `site/reports/smoke/macos/rand-m5.local/20260808T203610076893Z-pid39023-crosshost-ack/`; all 160 rows pass: ten repetitions of local guards plus exact two-way required-message and acknowledgement-ID/readback proof. |
| Benchmark/report | `just benchmark`; `just benchmark-report` | BLOCKED | `just benchmark` stopped before it created an isolated runtime: this active OS user was not declared dedicated (`ATM_CAPACITY_ISOLATED_OS_USER=1`) and had no explicit backup/restore authority (`ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE=1`). `just benchmark-report` was not run. |

## Blocker

The isolated `m5-test@atm-dev` mailbox, explicit M4 CLI path, and merged
cross-host ACK-routing fix are valid on the selected beta-ai-1 pair. G3 through
G6 now pass with retained evidence. The master report index links all current
M5 smoke artifacts at `site/reports/index.html`, including the complete G6
directory above.

G7 remains blocked only by the benchmark runner's explicit host-state safety
guard. No benchmark daemon, database, or report was created, and no active
daemon state was altered to bypass the guard. The M4 owner has been sent the
first-failing-row report and must provide an authorized isolated benchmark
environment or backup/restore authority before this PR can be marked ready.
