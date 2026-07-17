# AG.15 Mac Studio Bring-Up And Execution Plan

## Purpose

This document is the operator setup plan for Sprint AG.15's other-Mac cross-host
smoke rows using a Mac Studio as the second host.

It does not change AG.15 scope or acceptance criteria. It exists to make the
existing `AG-VAL-021A` through `AG-VAL-021F` rows executable against the
designated two-Mac topology:

- sender/control host: this Mac running `arch-ctm`
- receiver/second host: the Mac Studio

## Authoritative Control Surface

Use the shipped durable cross-host control plane, not the historical
workspace-config fallback.

Authoritative sources:

- AG.4 durable interface control plane:
  [sprint-AG4.md](./sprint-AG4.md)
- AG.5 host authorization control plane:
  [sprint-AG5.md](./sprint-AG5.md)
- accepted interface ADR:
  [ADR-028](../../adr/ADR-028-cross-host-interface-control-plane.md)
- accepted allowlist ADR:
  [ADR-029](../../adr/ADR-029-cross-host-host-authorization.md)
- AG.11 completion note removing the legacy listener-routing dependence:
  [ag11-completion-report.md](./ag11-completion-report.md)

Current shipped CLI surface on this branch:

- interface rows:
  - `atm daemon interfaces add`
  - `atm daemon interfaces update`
  - `atm daemon interfaces enable`
  - `atm daemon interfaces disable`
  - `atm daemon interfaces remove`
  - `atm daemon interfaces list`
- inbound host allowlist rows:
  - `atm daemon hosts allow`
  - `atm daemon hosts deny`
  - `atm daemon hosts remove`
  - `atm daemon hosts list`

Current doctor guidance on this branch also points operators to the durable CLI
surface:

- empty listener config remediation:
  `atm daemon interfaces add ...`
- empty allowlist remediation:
  `atm daemon hosts allow <host>`

Operational rule for AG.15:

- do not use `.atm.toml [daemon].peer_listen_addr` as the primary AG.15
  operator surface
- configure the Mac Studio and this Mac through durable interface rows plus
  durable allowed-host rows

## Topology Assumption

AG.15's goal is the simplest reachable Mac-to-Mac path first.

Assume:

- this Mac and the Mac Studio are on the same reachable LAN or same directly
  reachable local subnet
- no VPN hop is required for the first AG.15 pass
- both hosts can open TCP connections directly to each other's selected
  listener address on the chosen port

For the first run, use one ordinary LAN address per host and a single shared
listener port:

- this Mac: `<this-mac-ip>:43101`
- Mac Studio: `<mac-studio-ip>:43101`

Do not start with:

- multiple interface rows
- routed/VPN paths
- hostname aliases
- reverse-DNS assumptions

AG.5's actual shipped authorization token is the remote socket IP literal, not
reverse-DNS output. Use the peer host IPs in the allowlist commands.

## Mac Studio Prerequisites

The Mac Studio currently has no running ATM daemon. Bring-up is a clean start.

Before any AG.15 row runs on the Mac Studio, ensure:

1. source checkout
   - the Mac Studio has the same `atm-core` branch or a transferred build from
     the same candidate line as this worktree
   - target branch for AG.15 execution: `feature/pAG-s15-othermac-smoke`
2. toolchain if building locally
   - Rust toolchain present (`rustc`, `cargo`)
   - enough disk space to build the workspace
3. binary availability
   - either build locally on the Mac Studio:
     - `cargo build --release -p atm`
     - `cargo build --release -p atm-daemon`
   - or transfer matching built binaries from this Mac
4. ATM runtime identity setup
   - the Mac Studio has its intended ATM identity/team environment set before
     daemon bring-up
   - the Mac Studio receiver identity is known ahead of the smoke rows so the
     sender targets are exact

Recommended preference:

- first choice: build locally on the Mac Studio from the same branch head
- second choice: transfer already-built release binaries only if the operator
  cannot build locally

## Pre-Bring-Up Data To Collect

Before starting the Mac Studio daemon, collect:

- this Mac's reachable LAN IP
- Mac Studio's reachable LAN IP
- sender identity on this Mac
- receiver identity on the Mac Studio
- the exact AG.15 candidate port (`43101` unless a conflict is discovered)

These values feed both:

- durable interface rows
- durable allowlist rows

## Durable Configuration Plan

### 1. Configure a listener interface row on the Mac Studio

On the Mac Studio, add one LAN row using the Mac Studio's reachable LAN IP for
both bind and advertise addresses:

```bash
atm daemon interfaces add macstudio-lan \
  --bind-addr <mac-studio-ip> \
  --advertise-addr <mac-studio-ip> \
  --port 43101 \
  --kind lan
```

Then inspect:

```bash
atm daemon interfaces list --json
```

Expected result:

- exactly one enabled LAN row exists for the Mac Studio listener
- the row shows the selected bind/advertise address and port

### 2. Configure the Mac Studio allowlist for this Mac

On the Mac Studio, allow the sender host IP of this Mac:

```bash
atm daemon hosts allow <this-mac-ip> --note "AG15 sender host"
```

Then inspect:

```bash
atm daemon hosts list --json
```

Expected result:

- an enabled row exists for `<this-mac-ip>`
- the note is visible if requested

### 3. Configure the reciprocal rows on this Mac

On this Mac, add the sender-side listener row if it is not already present for
the AG.13/AG.14 ladder:

```bash
atm daemon interfaces add this-mac-lan \
  --bind-addr <this-mac-ip> \
  --advertise-addr <this-mac-ip> \
  --port 43101 \
  --kind lan
```

Then allow the Mac Studio IP on this Mac:

```bash
atm daemon hosts allow <mac-studio-ip> --note "AG15 Mac Studio peer"
```

Inspect both surfaces:

```bash
atm daemon interfaces list --json
atm daemon hosts list --json
```

### 4. Doctor confirmation before cross-host rows

Run on each host:

```bash
atm doctor --json
```

Expected doctor posture before AG.15 rows:

- no warning that cross-host listener configuration is empty
- no warning that the allowlist is empty
- no degraded bind warning for the chosen row

If doctor still recommends `atm daemon interfaces add ...` or
`atm daemon hosts allow ...`, stop and correct that host's durable rows before
running AG.15.

## Clean Daemon Bring-Up On The Mac Studio

Because nothing is currently running on the Mac Studio, no stop/restart dance is
required for an existing daemon owner.

Bring-up sequence on the Mac Studio:

1. ensure the interface row and allowlist row exist
2. start the daemon from the AG.15 candidate line
3. confirm the daemon binds the configured LAN row
4. run `atm doctor --json`
5. retain startup transcript and daemon log location

Operator note:

- use the normal daemon startup entrypoint already used elsewhere in ATM
  dogfooding on macOS
- this pass is a clean start, not a migration from an already-running daemon

Success criteria for Mac Studio bring-up:

- one daemon process is running
- its cross-host listener is bound using the durable interface row
- doctor does not show empty-listener or empty-allowlist findings

## AG-VAL-021A Through AG-VAL-021F Execution Map

The rows in `cross-host-smoke-checklist.md` map to the Mac Studio topology as
follows.

### AG-VAL-021A — unauthorized rejection

Goal:

- prove the Mac Studio rejects a non-allowlisted sender before mailbox mutation

Execution shape:

1. temporarily ensure this Mac's sender IP is not enabled on the Mac Studio
   allowlist
2. send from this Mac to the Mac Studio receiver target
3. collect:
   - sender result JSON/transcript
   - Mac Studio daemon rejection log
   - proof no receiver mailbox mutation occurred on the Mac Studio

### AG-VAL-021B — authorized durable send

Goal:

- prove the durable send succeeds once the allowlist row is enabled

Execution shape:

1. enable or re-allow `<this-mac-ip>` on the Mac Studio
2. send from this Mac to the Mac Studio receiver target
3. collect:
   - sender JSON
   - Mac Studio transcript
   - daemon logs from both Macs

### AG-VAL-021C — receiver read

Goal:

- prove the Mac Studio receiver can read the message delivered by `AG-VAL-021B`

Execution shape:

1. run `atm read --all --json` on the Mac Studio
2. verify the just-delivered message is present
3. retain receiver JSON/transcript and logs

### AG-VAL-021D — ack round-trip

Goal:

- prove `--requires-ack` plus reply-state mutation across the Mac-to-Mac pair

Execution shape:

1. send from this Mac with `--requires-ack`
2. acknowledge from the Mac Studio receiver
3. verify the original sender on this Mac observes the reply-state mutation
4. retain sender/receiver JSON and both daemon logs

### AG-VAL-021E — nudge/notification classification

Goal:

- prove durable delivery remains successful even when notification behavior
  degrades on the Mac-to-Mac path

Execution shape:

1. run one successful authorized send across the Mac pair
2. induce or observe the notification-path failure condition called for by the
   row
3. verify delivery remains classified as success while notification degradation
   remains visible
4. retain:
   - sender JSON
   - notification evidence
   - both daemon logs

### AG-VAL-021F — retry-visible recovery

Goal:

- prove temporary peer interruption or restart remains observable without
  losing result classification

Execution shape:

1. establish the normal authorized Mac-to-Mac path
2. during the send flow, introduce the row's temporary peer interruption
   (for example daemon restart or bounded peer unavailability)
3. verify recovery remains visible and correctly classified
4. retain:
   - transcript
   - both daemon logs
   - operator recovery notes

## First-Line Recovery Notes

Use these before classifying the row as a product bug.

### Firewall or connection refusal

Symptoms:

- send cannot connect to `<peer-ip>:43101`
- doctor or daemon logs show bind success on one side but connection failure on
  the other

First checks:

1. confirm both Macs are on the same reachable subnet
2. confirm the exact LAN IPs being used are still current
3. confirm the selected port is `43101` on both sides
4. confirm the daemon on the receiving side is actually running
5. rerun:
   - `atm daemon interfaces list --json`
   - `atm doctor --json`

### Wrong interface row

Symptoms:

- doctor shows degraded bind
- `last_bind_error` is populated
- binds are pointed at an old or non-local address

First checks:

1. inspect the row:
   - `atm daemon interfaces list --json`
2. correct the row with:
   - `atm daemon interfaces update ...`
   - or remove and re-add if the key itself is wrong
3. restart the daemon
4. rerun `atm doctor --json`

### Allowlist mismatch

Symptoms:

- the receiver rejects traffic that should be authorized

First checks:

1. inspect the receiving host's allowlist:
   - `atm daemon hosts list --json`
2. verify the row matches the sender's actual socket IP literal
3. if missing or disabled, run:
   - `atm daemon hosts allow <sender-ip>`
4. rerun the row

### Operator targeting error

Symptoms:

- send succeeds to the wrong identity or no expected receiver evidence appears

First checks:

1. verify sender target identity/team spelling
2. verify the Mac Studio receiver identity is the intended one for AG.15
3. verify the receiver-side read command is being run on the correct host and
   identity

## Evidence Retention Plan

For each AG.15 row retain:

- sender JSON or transcript
- receiver JSON or transcript where applicable
- daemon logs from this Mac
- daemon logs from the Mac Studio
- any notification or recovery side evidence required by the row

Store retained evidence under the existing AG.15 artifact location once the
rows are actually executed.

## Explicit Non-Goals For This Setup Pass

This document does not:

- execute the Mac Studio bring-up
- change AG.15 acceptance criteria
- introduce a new configuration surface
- switch AG.15 to Windows/macOS validation
- restore the old `.atm.toml [daemon].peer_listen_addr` operator path
