# ADR-031 — Remote-Target Contract And Cross-Host Dispatch Boundary

| Field | Value |
| --- | --- |
| ID | ADR-031 |
| Status | Proposed |
| Scope | Repository-wide |
| Deciders | ATM maintainers |
| Relates to | ADR-003, ADR-028, ADR-029, ADR-030, AG-FIND-005, Phase AG |

## Context

Early AG execution and later review proved that ordinary `atm send` still
lacked a first-class remote-target contract. The code could have a peer
transport surface and still silently fall through to local mailbox mutation
because remote target selection was not a typed, production-wired boundary.

The user-approved correction is intentionally narrow:

- cross-host sending must remain daemon-to-daemon
- remote-target parsing must be explicit and operator-visible
- localhost and the sender's own IP address must be ordinary remote-host
  values, not a special loopback-only mode
- host-routing logic must stop at one typed boundary so socket logic does not
  leak through general send/runtime code

## Decision

ATM will support exactly two operator-facing remote-send forms:

- `atm send <agent>@<team>.<host> ...`
- `atm send <agent>@<team> --host <host> ...`

Those two forms normalize into one typed remote-host field on the send request.

Parser and normalization rules:

- agent/member names must not contain `.`
- team names must not contain `.`
- inline parsing splits on the final `.` after `@`
- the suffix after that final `.` is the remote host
- the prefix before that final `.` is the team name
- mixed inline-host plus `--host` input is rejected instead of silently
  preferring one source

Dispatch rules:

- empty normalized remote-host field => local mailbox path
- non-empty normalized remote-host field => cross-host delivery boundary
- sender-side daemons must not write a remote target directly into a local
  mailbox path
- `localhost` and the sender host's own advertised or bound IP address are
  ordinary non-empty remote-host values on that same cross-host delivery path

Delivery-result rules:

- if the cross-host path is healthy, the CLI may wait up to `10s` for remote
  daemon acceptance
- if the cross-host path is unhealthy, the CLI returns immediately with a
  deferred-delivery result
- the daemon may continue bounded background retry for `60s..120s`
- the daemon appends the final delivery/failure receipt into the sender inbox

Boundary rule:

- parsing/normalization/classification ends at one typed remote-target
  contract
- socket and transport implementation details stay behind the cross-host
  delivery boundary and its storage/config boundaries

## Consequences

- AG.11 owns the typed remote-target contract and production wiring proof
- AG.12 and AG.13 prove that localhost and self-IP use the ordinary remote-host
  path
- AG.14 must include at least one ADR-003 Tier-3 regression that exercises the
  production cross-host delivery path end to end
- any future host-routing change must update this ADR instead of silently
  widening the accepted CLI surface
