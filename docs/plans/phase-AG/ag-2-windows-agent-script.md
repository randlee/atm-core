# AG.2 Windows-Agent Cooperative Execution Script

## Purpose

This script is the operator-facing execution order for `AG.2`.

Use it together with:

- `docs/plans/phase-AG/sprint-AG2.md`
- `docs/plans/phase-AG/cross-host-setup-runbook.md`
- `docs/plans/phase-AG/cross-host-smoke-checklist.md`

This document does not replace those sources. It turns them into an exact
Windows/macOS turn-taking procedure so `windows-agent` and `arch-ctm` can run
`AG-VAL-003` through `AG-VAL-007` without improvising.

## 2026-07-15 VPN Address Correction And Windows Pull Notes

This section supersedes the earlier same-subnet example pair.

Important correction:

- there are no new daemon code changes in this note update
- Windows must pull because the repo-local AG.2 setup/docs changed
- Windows does not need to rebuild solely because of this note/config update
  if the existing AG.2 daemon binary already comes from the current
  `feature/cross-host-communication` code line

Current confirmed AG.2 host values:

- Windows host IP: `10.10.100.98`
- macOS route-reachable VPN IP: `10.212.36.11`
- listener port: `43101`

Required setup correction:

- `ATM_DAEMON_PEER_ADDR` is outbound-only
- inbound listener bind comes from `.atm.toml` `[daemon].peer_listen_addr`
- the daemon resolves `.atm.toml` from the current repo/worktree directory it
  is started in, not from the location of the built binary artifact
- this branch now carries the repo-local listener config:

```toml
[daemon]
peer_listen_addr = "0.0.0.0:43101"
```

Outbound peer targets for the current VPN lane:

- macOS:
  - `ATM_DAEMON_PEER_ADDR=10.10.100.98:43101`
- Windows:
  - `ATM_DAEMON_PEER_ADDR=10.212.36.11:43101`

Windows-agent exact next steps:

1. Pull `feature/cross-host-communication` from remote.
2. Confirm repo-local `.atm.toml` now contains:
   - `[daemon]`
   - `peer_listen_addr = "0.0.0.0:43101"`
3. Keep the current AG.2 daemon build if it already came from this branch
   state; no rebuild is required for this note/config-only correction.
4. Restart the Windows AG.2 daemon from the repo root with:
   - `ATM_DAEMON_PEER_ADDR=10.212.36.11:43101`
5. Before attempting AG rows, capture:
   - daemon startup transcript
   - `atm doctor --json`
   - `Test-NetConnection 10.212.36.11 -Port 43101`
6. Report back with:
   - daemon PID
   - whether `Test-NetConnection 10.212.36.11 -Port 43101` succeeded
   - whether `atm doctor --json` remained healthy/ready

macOS paired operator action:

1. Start/restart the AG.2 daemon from this same branch/worktree directory.
2. If the built daemon artifact lives under another worktree
   (for example `integrate/phase-AG/target/debug/atm-daemon`), that is fine,
   but the process must still be launched with
   `feature/cross-host-communication` as the current working directory so it
   reads this branch's `.atm.toml`.
3. Use:
   - `ATM_DAEMON_PEER_ADDR=10.10.100.98:43101`
4. Verify listener presence with:
   - `lsof -nP -iTCP:43101 -sTCP:LISTEN`
   - expected result for the VPN lane: `*:43101` or `0.0.0.0:43101`, not a
     single LAN-only bind such as `192.168.128.82:43101`
5. If macOS is bound only to a LAN address, stop and fix the start location
   before asking Windows to retry connectivity. That is a local startup/config
   mistake, not a Windows routing failure.

## Repo Root Convention

All paths in this script are relative to the repository root.

Assume the operator has already changed directory to the repo root before
following any step.

Examples in this script therefore use paths such as:

- `docs/plans/phase-AG/cross-host-smoke-checklist.md`
- `artifacts/`
- `logs/`

Never substitute absolute host-specific paths into the procedure.

## Code And Tool Mapping

This script is intentionally tied to the code and operator tools that exist in
the current AG line.

Cross-host dialing currently maps to these product surfaces:

- operator env:
  - `ATM_DAEMON_PEER_ADDR`
- daemon config parsing:
  - `crates/atm-daemon/src/peer_transport.rs`
  - `crates/atm-daemon/src/composition.rs`
- daemon startup / runtime lane:
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm-daemon/src/peer_transport.rs`

Core row commands currently map to these CLI surfaces:

- durable send:
  - `atm send --json`
  - implementation entry: `crates/atm/src/commands/send.rs`
- receiver-side read:
  - `atm read --all --json`
  - implementation entry: `crates/atm/src/commands/read.rs`
- ack mutation:
  - `atm ack`
  - implementation entry: `crates/atm/src/commands/ack.rs`
- required-ack send contract:
  - `atm send --json --requires-ack`
  - documented behavior also referenced in `crates/atm/src/commands/help.rs`

Diagnostic cross-check surfaces for failures:

- `atm doctor --json`
- daemon runtime logs
- peer transport retained log entries under the clean-room log directory

This script should be updated if those code surfaces change.

## Scope

This script covers only:

- `AG-VAL-003`
- `AG-VAL-004`
- `AG-VAL-005`
- `AG-VAL-006`
- `AG-VAL-007`

It does not cover:

- AG.1 clean-room bring-up
- AG.3 degraded/retry-visible validation
- AG.4 copied-state validation
- AG.5 final verdict work

## Preconditions

Do not start AG.2 until all of the following are true:

- `AG-VAL-001` is recorded
- `AG-VAL-002` is recorded
- AG.1 first-live-channel viability is resolved to either:
  - a working channel that allows AG.2 to proceed, or
  - a named blocking finding that explicitly prevents AG.2
- both operators are using the release-target `1.3.1` binaries
- both operators are in clean-room state per `cross-host-setup-runbook.md`
- both operators know the exact peer address values in use

If any precondition is false, stop and record a blocker instead of attempting
AG.2 rows.

## Roles

- `windows-agent`
  - executes all Windows-side commands
  - retains Windows-side artifacts
  - confirms receiver-side results when Windows is the receiver
- `arch-ctm`
  - executes all macOS-side commands
  - retains macOS-side artifacts
  - confirms receiver-side results when macOS is the receiver
- `quality-mgr`
  - reviews retained evidence after rows complete or fail

## Shared Operating Rules

For every row:

1. Name the row before running commands.
2. Do not overlap rows.
3. Sender records sender-side artifacts immediately after the send attempt.
4. Receiver confirms receipt/read outcome before the next row begins.
5. If a row fails, stop and create a finding before retrying or widening scope.

Do not “batch” multiple rows and reconstruct evidence later.

## Artifact Rule Per Row

For every row, retain:

- sender command transcript
- receiver command transcript
- sender JSON result when applicable
- receiver JSON result when applicable
- relevant daemon log snapshot from both hosts after the row
- exact row outcome: `PASS` or named finding

If the failure is ambiguous, retain artifacts first and classify second.

Recommended artifact layout relative to repo root on each host:

- `artifacts/phase-AG/<row-id>/sender/`
- `artifacts/phase-AG/<row-id>/receiver/`
- `artifacts/phase-AG/<row-id>/logs/`

Minimum retained files per row:

- `sender-command.txt`
- `sender-result.json`
- `receiver-command.txt`
- `receiver-result.json`
- `sender-daemon-log.txt`
- `receiver-daemon-log.txt`
- `row-verdict.md`

If a row does not naturally produce one of those files, create the closest
equivalent transcript and record that substitution in `row-verdict.md`.

## Command Shape Rule

Use the native ATM CLI only.

Do not replace the documented row commands with:

- wrapper scripts
- direct database edits
- ad hoc helper programs
- manual mailbox file edits

For AG.2 the authoritative operator actions are:

- `atm send --json`
- `atm read --all --json`
- `atm ack`
- `atm doctor --json`

## Message Identity Rule

For rows `AG-VAL-003` through `AG-VAL-007`, every sender must put a unique
row marker in the sent message body so the receiver can identify the expected
message deterministically.

Required pattern:

- include the row id
- include sender host
- include receiver host
- include an attempt number if retried after a finding is cleared

Example body fragment:

- `AG-VAL-003 windows->macOS attempt-1`

Do not rely on timestamp-only identification.

## Row Execution Order

Run rows in this exact order:

1. `AG-VAL-003`
2. `AG-VAL-005`
3. `AG-VAL-004`
4. `AG-VAL-006`
5. `AG-VAL-007`

Reason:

- each read row immediately validates the preceding send row
- the ack round-trip runs only after both directional send/read flows are real

## Row Script

### Row `AG-VAL-003` — Windows -> macOS durable send

Owner:

- sender: `windows-agent`
- receiver confirmation: `arch-ctm`

Procedure:

1. `windows-agent` announces start of `AG-VAL-003`.
2. `windows-agent` runs the Windows-side `atm send --json` command targeting the
   macOS recipient, with a body containing the required row marker.
3. `windows-agent` saves:
   - full command transcript
   - JSON result
   - Windows daemon log snapshot
4. `arch-ctm` confirms whether receiver-side arrival evidence exists for the
   exact row marker sent in step 2.
5. If the send fails or delivery is unclear, stop and open a finding.

PASS condition:

- Windows sender result is successful
- macOS-side evidence supports that the durable send reached the receiver side

If `AG-VAL-003` does not pass, do not continue to `AG-VAL-005`.

### Row `AG-VAL-005` — macOS reads the Windows -> macOS message

Owner:

- sender-side reference: prior row `AG-VAL-003`
- receiver: `arch-ctm`

Procedure:

1. `arch-ctm` announces start of `AG-VAL-005`.
2. `arch-ctm` runs `atm read --all --json` on macOS.
3. `arch-ctm` saves:
   - full command transcript
   - JSON result
   - macOS daemon log snapshot
4. `arch-ctm` confirms whether the just-sent Windows message with the
   `AG-VAL-003` row marker is present and identifiable.
5. `windows-agent` retains the original sender JSON from `AG-VAL-003` as the
   linkage artifact.

PASS condition:

- macOS read returns the just-delivered Windows-originated message

If this fails, open a finding before attempting reverse direction.

### Row `AG-VAL-004` — macOS -> Windows durable send

Owner:

- sender: `arch-ctm`
- receiver confirmation: `windows-agent`

Procedure:

1. `arch-ctm` announces start of `AG-VAL-004`.
2. `arch-ctm` runs the macOS-side `atm send --json` command targeting the
   Windows recipient, with a body containing the required row marker.
3. `arch-ctm` saves:
   - full command transcript
   - JSON result
   - macOS daemon log snapshot
4. `windows-agent` confirms whether receiver-side arrival evidence exists as
   expected for the row.
5. If the send fails or delivery is unclear, stop and open a finding.

PASS condition:

- macOS sender result is successful
- Windows-side evidence supports that the durable send reached the receiver side

If `AG-VAL-004` does not pass, do not continue to `AG-VAL-006`.

### Row `AG-VAL-006` — Windows reads the macOS -> Windows message

Owner:

- sender-side reference: prior row `AG-VAL-004`
- receiver: `windows-agent`

Procedure:

1. `windows-agent` announces start of `AG-VAL-006`.
2. `windows-agent` runs `atm read --all --json` on Windows.
3. `windows-agent` saves:
   - full command transcript
   - JSON result
   - Windows daemon log snapshot
4. `windows-agent` confirms whether the just-sent macOS message with the
   `AG-VAL-004` row marker is present and identifiable.
5. `arch-ctm` retains the original sender JSON from `AG-VAL-004` as the
   linkage artifact.

PASS condition:

- Windows read returns the just-delivered macOS-originated message

If this fails, open a finding before attempting `AG-VAL-007`.

### Row `AG-VAL-007` — cross-host `--requires-ack` round-trip

Owner:

- sender: `arch-ctm`
- receiver ack actor: `windows-agent`
- final sender-side confirmation: `arch-ctm`

Procedure:

1. `arch-ctm` announces start of `AG-VAL-007`.
2. `arch-ctm` runs `atm send --json --requires-ack` targeting the Windows
   recipient, with a body containing the required row marker.
3. `arch-ctm` saves:
   - send transcript
   - sender JSON result
   - macOS daemon log snapshot after send
4. `windows-agent` runs:
   - `atm read --all --json` to confirm the message is present
   - `atm ack ...` to acknowledge it
5. `windows-agent` saves:
   - read transcript
   - ack transcript / JSON result
   - Windows daemon log snapshot after ack
6. `arch-ctm` runs the sender-side verification step needed to prove reply-state
   mutation is visible.
7. `arch-ctm` saves:
   - final verification transcript / JSON result
   - macOS daemon log snapshot after ack observation

Required sender-side verification rule:

- verify against the exact message created in step 2
- capture the message id if the send output exposes it
- use the message id or unique row marker when confirming the reply-state
  mutation

Do not close `AG-VAL-007` on a generic “ack succeeded” statement alone.

PASS condition:

- required-ack send succeeds
- Windows receiver can read and ack the message
- macOS sender can observe the reply-state mutation on the original message

If any sub-step fails, record one named finding tied to `AG-VAL-007`.

## Stop Rules

Stop immediately if any of the following occur:

- sender JSON result is ambiguous or missing
- receiver cannot identify the expected message deterministically
- daemon crashes
- daemon reports degraded state not already expected for this sprint
- operator must guess a command, path, env value, or peer address

When stopping:

1. preserve artifacts
2. classify the failure using the AG runbook categories
3. open a named finding
4. do not continue to the next row until the finding is accepted as the row
   outcome or explicitly cleared

If the stop reason appears to be peer transport configuration or dialing, check
and retain the exact values used for:

- `ATM_DAEMON_PEER_ADDR`
- the clean-room daemon startup command
- any configured listener address surfaced by the current AG.1 code path

## Minimal Operator Handshake Template

Use this short coordination pattern for every row:

1. sender: `starting AG-VAL-00X`
2. sender: `send complete, artifacts saved`
3. receiver: `receiver confirmation complete` or `receiver failed`
4. if failed: `stop, finding required`
5. if passed: `row PASS, proceed`

This keeps AG.2 execution serialized and auditable.

## Completion Rule

AG.2 is complete only when every row from `AG-VAL-003` through `AG-VAL-007`
ends as either:

- `PASS`, or
- a named evidence-backed finding

Do not say AG.2 is complete merely because commands were attempted.
