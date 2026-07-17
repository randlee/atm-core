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
- on `1.3.1`, treat this as team/config isolation only unless the active host
  daemon has been intentionally replaced for the validation lane

Current `1.3.1` constraint observed during AG.1:

- `ATM_HOME` and `ATM_CONFIG_HOME` isolate team/config discovery surfaces
- they do not relocate the host daemon singleton, owner lock, durable SQLite
  store, or default retained log sink
- Lane A therefore cannot honestly claim daemon-runtime isolation on one host
  unless the active host daemon is itself the only daemon under test
- AG.1 records this as a setup-contract finding rather than pretending a
  second clean-room daemon is supported

Important implementation constraint discovered during AG.1 Windows setup:

- the `1.3.1` runtime derives daemon singleton and durable SQLite state from
  the OS-account home via `current_host_runtime_scope()`, not from `ATM_HOME`,
  `ATM_CONFIG_HOME`, `HOME`, or `USERPROFILE`
- under a normal operator account, release binaries therefore still use the
  host-scoped `.atm/db/mail.db` even when `ATM_HOME`, `ATM_CONFIG_HOME`, and
  `ATM_LOG_DIR` point at disposable directories
- do not claim a strict no-live-OS-account-state clean-room PASS from a normal
  account until the team has either provided an approved disposable
  OS-account/container/VM isolation procedure or added an approved
  release-binary durable-state override
- for AG.1 Windows setup only, team-lead/windows-operator accepted using this
  computer's host account environment because no installed/running ATM service
  exists on the computer; reports using this exception must call it
  `host-env` evidence, not strict clean-room evidence

Minimal team bootstrap required before same-host commands:

1. create `.claude/teams/<team>/config.json` under the disposable `ATM_HOME`
   with at least `{"members":[]}`
2. run `atm teams add-member <team> <member> ... --json` for each clean-room
   member needed by the row
3. only then run `atm doctor --json`, `atm list --json`, `atm clear --json`,
   `atm send ... --json`, or `atm read ... --json`

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
- whether the validation is using the one active host daemon or an explicitly
  replaced host daemon instance

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

## Patched-Daemon Demo Contract For `AG-FIND-004`

Use this section when validating the patched daemon from PR #551 / commit
`69395061` rather than the released `1.3.1` binaries.

The patched cross-host lane now requires two distinct settings per host:

- outbound peer target:
  - `ATM_DAEMON_PEER_ADDR=<remote-host-ip:port>`
- inbound peer listener bind:
  - `.atm.toml`:
    ```toml
    [daemon]
    peer_listen_addr = "<local-host-ip:port>"
    ```

Rules:

- `ATM_DAEMON_PEER_ADDR` tells the local daemon where to dial
- `daemon.peer_listen_addr` tells the local daemon what local TCP address to
  bind and accept on
- the daemon loads `.atm.toml` from the repo/worktree directory it is launched
  in; if the operator runs a binary built in another worktree, the process must
  still start with the intended repo root as its current working directory
- for AG.2 operator setup, prefer wildcard listener bind
  `0.0.0.0:43101` unless a host policy requires a narrower literal interface
  bind
- both values must be literal `IP:port`
- the two hosts must use opposite values:
  - Windows `.atm.toml` listener address = address macOS uses in
    `ATM_DAEMON_PEER_ADDR`
  - macOS `.atm.toml` listener address = address Windows uses in
    `ATM_DAEMON_PEER_ADDR`

### VPN / multi-interface address selection rule

Do not assume both hosts share the same IPv4 subnet.

For AG.2 the outbound peer target must be the remote host address that is
actually reachable on the route used between the two hosts, not a guessed LAN
address on the same-looking local subnet.

Operator procedure:

1. determine the route to the remote host IP
2. identify the local interface/address used on that route
3. bind the local listener on `0.0.0.0:43101` unless a narrower bind is
   required
4. set `ATM_DAEMON_PEER_ADDR` to the remote host's route-reachable
   `IP:43101`

Current AG.2 VPN lane example:

- Windows host IP: `10.10.100.98`
- macOS route to Windows uses `utun6`
- macOS VPN address on that route: `10.212.36.11`
- therefore:
  - Windows should dial `10.212.36.11:43101`
  - macOS should dial `10.10.100.98:43101`

Useful verification commands:

- macOS:
  - `route -n get <windows-ip>`
  - `ifconfig <reported-interface>`
  - `lsof -nP -iTCP:43101 -sTCP:LISTEN`
- Windows:
  - `route print <mac-ip>`
  - `Test-NetConnection <mac-ip> -Port 43101`

2026-07-15 operator finding:

- macOS produced a false cross-host blocker when the daemon binary from
  `integrate/phase-AG/target/debug/atm-daemon` was started with
  `integrate/phase-AG` as the working directory instead of
  `feature/cross-host-communication`
- result:
  - outbound env was correct
  - inbound listener config from the AG worktree `.atm.toml` was not used
  - the daemon bound only to `192.168.128.82:43101`
  - Windows correctly failed `Test-NetConnection 10.212.36.11 -Port 43101`
- fix:
  - start the daemon with `feature/cross-host-communication` as the current
    working directory
  - it is acceptable to execute a binary artifact located under another
    worktree as long as the working directory is the AG worktree containing the
    intended `.atm.toml`

### Minimal macOS / Windows demo pair

Example pair:

- macOS listener: `192.168.1.20:43101`
- Windows listener: `192.168.1.21:43101`

Then configure:

- on macOS:
  - `.atm.toml`:
    ```toml
    [daemon]
    peer_listen_addr = "192.168.1.20:43101"
    ```
  - env:
    - `ATM_DAEMON_PEER_ADDR=192.168.1.21:43101`

- on Windows:
  - `.atm.toml`:
    ```toml
    [daemon]
    peer_listen_addr = "192.168.1.21:43101"
    ```
  - env:
    - `ATM_DAEMON_PEER_ADDR=192.168.1.20:43101`

### First live demo command order

1. update both hosts to commit `69395061`
2. write `.atm.toml` with the host-local `daemon.peer_listen_addr`
3. start macOS patched daemon
4. start Windows patched daemon
5. capture `atm doctor --json` on both hosts
6. from Windows, run the first durable send to the macOS recipient
7. on macOS, run `atm read --all --json`
8. if send succeeds, immediately retain:
   - sender JSON result
   - receiver read JSON result
   - retained logs from both hosts

Runbook discipline note:

- do not apply exploratory code patches during AG execution just to "see what
  happens"
- a named finding fix is different: once a concrete ledger-linked defect exists
  (for example `AG-FIND-004`) the deliberate implementation patch belongs in
  normal branch/PR history with the finding id called out in the evidence trail

## Localhost Remote-Target Operator Path (AG.12)

Use this section for the AG.12 same-host localhost proof lane only.

Scope:

- one host
- one daemon runtime
- two ATM identities on that same host
- remote-target addressing through `localhost`, not through the local mailbox
  path

Required behavior:

- both supported localhost syntaxes must normalize onto the same peer-transport
  dispatch path:
  - `<agent>@<team>.localhost`
  - `<agent>@<team> --host localhost`
- localhost traffic must still obey host authorization rules before mailbox
  mutation
- the localhost proof must not use any special loopback bypass; it must use the
  same daemon peer-listener machinery as any other non-empty remote host

Minimal localhost setup:

1. use the AG.12 branch/worktree
2. prepare disposable `ATM_HOME`, `ATM_CONFIG_HOME`, and `ATM_LOG_DIR`
3. create the team bootstrap config and add at least:
   - one sender identity
   - one receiver identity
4. configure the daemon peer listener for same-host use
   - the simplest supported bind is:
     ```toml
     [daemon]
     peer_listen_addr = "127.0.0.1:43101"
     ```
5. run the daemon from the repo root that contains the intended `.atm.toml`
6. for unauthorized rejection proof, leave localhost absent from the allowlist
7. for success proof, explicitly allow localhost and rerun the same remote-target
   send/read/ack path

Operational checks:

- `atm doctor --json`
  - verify the daemon is healthy enough to serve the localhost listener
- `atm send --json ... --host localhost`
  - use this when the operator wants the hostname separated from the mailbox
    address
- `atm send --json <agent>@<team>.localhost ...`
  - use this when the operator wants the inline remote-target syntax

Expected localhost troubleshooting outcomes:

- if localhost is not allowlisted, the daemon must reject before mailbox
  mutation
- if localhost is allowlisted and the listener is healthy, send/read/ack should
  succeed through the peer listener
- if secure mode is enabled, the localhost proof currently uses pinned
  fingerprint trust per ADR-032 rather than full X.509 chain validation

Retained AG.12 evidence recorded on Friday, July 17, 2026:

- unauthorized localhost rejection proof:
  - `docs/plans/phase-AG/artifacts/ag12/ag-val-016-unauthorized.txt`
- localhost send/read proof:
  - `docs/plans/phase-AG/artifacts/ag12/ag-val-017-send-read.txt`
- localhost ack/reply-state proof:
  - `docs/plans/phase-AG/artifacts/ag12/ag-val-017-ack.txt`
- localhost secured-transport acceptance proof:
  - `docs/plans/phase-AG/artifacts/ag12/ag-val-018-secure-roundtrip.txt`
- localhost secured-transport mismatch rejection proof:
  - `docs/plans/phase-AG/artifacts/ag12/ag-val-018-fingerprint-mismatch.txt`

## Self-IP Remote-Target Operator Path (AG.13)

Use this section for the AG.13 same-host self-IP proof lane only.

Scope:

- one host
- one daemon runtime
- two ATM identities on that same host
- remote-target addressing through the host's own advertised or bound non-loopback
  IP address

Required behavior:

- both supported self-IP syntaxes must normalize onto the same peer-transport
  dispatch path:
  - `<agent>@<team>.<self-ip>`
  - `<agent>@<team> --host <self-ip>`
- self-IP traffic must still obey host authorization rules before mailbox
  mutation
- the self-IP proof must use a real daemon interface row for the host's own
  advertised or bound IP; do not relabel `localhost` or `127.0.0.1` evidence
  as self-IP

Minimal self-IP setup:

1. use the AG.13 branch/worktree
2. prepare disposable `ATM_HOME`, `ATM_CONFIG_HOME`, and `ATM_LOG_DIR`
3. create the team bootstrap config and add at least:
   - one sender identity
   - one receiver identity
4. identify one real non-loopback IP address that belongs to this host
   - use the same literal IP for both the daemon interface row and the
     remote-target send
5. create and enable one daemon interface row for that IP
   - example:
     ```bash
     atm daemon interfaces add en0 \
       --bind-addr 192.168.1.20 \
       --advertise-addr 192.168.1.20 \
       --port 43101 \
       --kind lan
     atm daemon interfaces enable en0 192.168.1.20 43101
     ```
6. start the daemon from the repo root that contains the intended `.atm.toml`
7. for unauthorized rejection proof, leave the self-IP absent from the
   allowlist
8. for success proof, explicitly allow the self-IP and rerun the same
   remote-target send/read/ack path
   - example:
     ```bash
     atm daemon hosts allow 192.168.1.20
     ```

Operational checks:

- `atm doctor --json`
  - verify the daemon reports the enabled self-IP interface row and healthy
    listener state
- `atm daemon interfaces list`
  - verify the exact bind/advertise IP and port that the daemon will use
- `atm send --json ... --host <self-ip>`
  - use this when the operator wants the hostname separated from the mailbox
    address
- `atm send --json <agent>@<team>.<self-ip> ...`
  - use this when the operator wants the inline remote-target syntax

Expected self-IP troubleshooting outcomes:

- if the self-IP is not allowlisted, the daemon must reject before mailbox
  mutation
- if the self-IP is allowlisted and the listener is healthy, send/read/ack
  should succeed through the peer listener
- if the daemon interface row is missing or disabled, the send must fail closed
  instead of silently falling back to the local mailbox path
- if secure mode is enabled, the self-IP proof uses the same pinned-fingerprint
  trust contract as the localhost proof until a later transport-security sprint
  changes that behavior

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
