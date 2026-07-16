# Phase AG Cross-Host Setup Runbook

## Purpose

This runbook defines the operator-owned setup contract for the first
Windows/macOS cross-host validation lane.

It is intentionally concrete enough that:

- the macOS operator can prepare one disposable daemon environment
- the Windows operator can prepare one disposable daemon environment
- both operators can point their daemons at each other
- both operators can attempt the first live cross-host channel without guessing

This runbook is for validation, not for live user-state migration.

Important scope note:

- this document records the historical early AG setup contract
- it intentionally preserves the env-driven bring-up that was used before the
  missing durable control-plane surface was fully understood
- after AG.4 / AG.5 land, this runbook must be revised to use the SQLite +
  CLI-managed interface and allowlist surfaces as the primary operator path
- `ATM_DAEMON_PEER_ADDR` is therefore transitional/historical in this document,
  not the desired final product surface

## Clean-Room Directory Contract

Each host creates three disposable directories:

- `ATM_HOME`
- `ATM_CONFIG_HOME`
- `ATM_LOG_DIR`

They must be disposable and isolated from live host state.

Required rule:

- do not point any of these at live `~/.atm`, live `~/.claude`, or any other
  retained user state during Lane A

## Binary Contract

Each host must use the release-target binaries for `1.3.1`:

- `atm`
- `atm-daemon`

Do not mix:

- source-tree debug binaries
- stale installed copies from another release
- wrapper scripts that hide which binary is actually being invoked

Each operator must record:

- exact `atm --version`
- exact resolved `atm` path
- exact resolved `atm-daemon` path
- whether the observed cross-host transport path is plain TCP or TLS-backed

## Required Environment Contract

Each host must export/set at least for the historical early-AG lane:

- `ATM_HOME=<clean-room path>`
- `ATM_CONFIG_HOME=<clean-room path>`
- `ATM_LOG_DIR=<clean-room path>`
- `ATM_DAEMON_PEER_ADDR=<peer-host-ip:port>`

Notes:

- `ATM_DAEMON_PEER_ADDR` was the early discovered peer-transport entry point
  used by `atm-daemon` for remote delivery attempts
- current product behavior parses `ATM_DAEMON_PEER_ADDR` as a literal
  `SocketAddr`, so operators must provide a literal `IP:port`, not a hostname
- both hosts must know the peer address value they are expected to dial
- if a host cannot determine the right peer address/port from product docs or
  observable config, that is a setup-contract finding
- AG.4 / AG.5 exist precisely because this env-driven contract is not an
  acceptable steady-state operator surface

## Transport-Security Contract

The documented architecture/requirements line says cross-host daemon transport
is TCP/TLS, not plain unauthenticated TCP. Phase AG must therefore capture one
explicit transport-security disposition before any release-usable verdict:

- if the release implementation presents real TLS evidence, retain it as part
  of the validation artifacts
- if the release implementation remains plain TCP, open or maintain a named
  `PRODUCT-BUG` / requirement-drift finding against the transport requirement
  and ensure any AG release-usable statement explicitly excludes
  transport-security coverage

## macOS Bring-Up Checklist

1. Confirm release binaries:
   - `atm --version`
   - `which atm`
   - `which atm-daemon`
2. Create clean-room directories.
3. Export clean-room variables plus `ATM_DAEMON_PEER_ADDR`.
4. Start the daemon with the literal release command:
   - `ATM_HOME="$ATM_HOME" ATM_CONFIG_HOME="$ATM_CONFIG_HOME" ATM_LOG_DIR="$ATM_LOG_DIR" ATM_DAEMON_PEER_ADDR="$ATM_DAEMON_PEER_ADDR" atm-daemon`
5. Record:
   - daemon PID
   - daemon startup transcript
   - `atm doctor --json`
6. Prove same-host baseline commands still work in clean-room state:
   - `atm doctor --json`
   - `atm list --json`
   - `atm clear --json`
   - `atm send --json ...` to a same-host target if the setup requires it
   - `atm read --all --json`

## Windows Bring-Up Checklist

1. Confirm release binaries:
   - `atm --version`
   - resolved executable path for `atm`
   - resolved executable path for `atm-daemon`
2. Create clean-room directories.
3. Set clean-room variables plus `ATM_DAEMON_PEER_ADDR`.
4. Start the daemon with the literal release command:
   - PowerShell:
     `$env:ATM_HOME='<clean-room path>'; $env:ATM_CONFIG_HOME='<clean-room path>'; $env:ATM_LOG_DIR='<clean-room path>'; $env:ATM_DAEMON_PEER_ADDR='<peer-host-ip:port>'; atm-daemon`
5. Record:
   - daemon PID
   - daemon startup transcript
   - `atm doctor --json`
6. Prove same-host baseline commands still work in clean-room state:
   - `atm doctor --json`
   - `atm list --json`
   - `atm clear --json`
   - `atm send --json ...` to a same-host target if the setup requires it
   - `atm read --all --json`

## First Live Channel Attempt

The first live cross-host attempt should happen as soon as both hosts have:

- healthy clean-room daemon bring-up
- healthy same-host release-binary proof
- explicit peer address values set

The first attempt is a viability probe for AG.1 only. It may open a finding,
but formal checklist ownership for `AG-VAL-003` through `AG-VAL-007` remains in
`AG.2`.

Recommended order:

1. bring up macOS daemon
2. bring up Windows daemon
3. capture `atm doctor --json` on both hosts
4. attempt one Windows -> macOS durable send
5. if successful, immediately confirm receiver-side read on macOS

If this fails, stop widening scope and record one finding before trying the
next row.

## Required Evidence Per Host

Each host must retain:

- binary version output
- resolved binary paths
- env values used for clean-room execution
- daemon start transcript
- daemon PID
- `atm doctor --json`
- command transcript for every executed row
- daemon log snapshot after each failed row and after each milestone pass

## Failure Classification

Classify the first failure before moving on:

- `SETUP-GAP`
  - the docs/runbook did not provide enough information to execute
  - also use this classification when the documented peer-address contract
    allowed a hostname where the actual product only accepts literal `IP:port`
- `ENV-MISTAKE`
  - operator input was wrong and the product behaved reasonably
- `PRODUCT-BUG`
  - operator input was correct and the product failed
- `EXTERNAL-BLOCKER`
  - network/firewall/certificate/host-policy issue prevented validation

Use the `classification` enum defined in `plan-phase-AG.md`. This runbook is a
consumer of that enum only.

## Immediate Recovery Rules

- do not patch code during Lane A setup just to “see what happens”
- do not move to copied-state before the clean-room channel is real
- do not widen to more rows while the first live channel attempt is unresolved
- every failure must get a finding ID before another workaround is attempted
