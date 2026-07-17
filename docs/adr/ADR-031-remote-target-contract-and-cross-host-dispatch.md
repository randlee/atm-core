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
- socket transport must reuse the same ATM wire message shapes already used on
  other ATM transports
- host-routing logic must stop at one typed boundary so socket logic does not
  leak through general send/runtime code

## Decision

ATM will support exactly two operator-facing remote-send forms:

- `atm send <agent>@<team>.<host> ...`
- `atm send <agent>@<team> --host <host> ...`

Those two forms normalize into one typed remote-host field on the send request.

Illustrative contract types:

```rust
pub struct RemoteTargetHost(String);

pub enum SendTargetParseError {
    MissingTeam,
    MissingHost,
    InvalidAgentNameDot,
    InvalidTeamNameDot,
    MixedInlineAndExplicitHost,
    MalformedInlineRemoteTarget,
}

pub enum RemoteDeliveryDecision {
    HealthyImmediateWait,
    DeferredRetry,
}

pub enum RemoteFailureKind {
    TransientConnectTimeout,
    TransientConnectionRefused,
    TransientConnectionReset,
    TransientHostUnreachable,
    TerminalAllowlistRejected,
    TerminalAuthenticationRejected,
    TerminalProtocolRejected,
    TerminalMalformedTarget,
}

pub enum SendOutcome {
    Delivered,
    Deferred { receipt_message_id: MessageId },
    RejectedTerminal(RemoteFailureKind),
    OutcomeUnknown,
}

pub enum CrossHostDeliveryInfraError {
    RuntimeUnavailable,
    StorageUnavailable,
    InternalInvariantViolation,
}
```

Parser and normalization rules:

- agent/member names must not contain `.`
- team names must not contain `.`
- inline parsing splits on the final `.` after `@`
- the suffix after that final `.` is the remote host
- the prefix before that final `.` is the team name
- mixed inline-host plus `--host` input is rejected instead of silently
  preferring one source
- each parser rejection maps to one stable error code / variant:
  - missing team => `MissingTeam`
  - missing host => `MissingHost`
  - dot in agent/member name => `InvalidAgentNameDot`
  - dot in team name => `InvalidTeamNameDot`
  - mixed inline-host plus `--host` => `MixedInlineAndExplicitHost`
  - malformed final-dot split / otherwise invalid inline form =>
    `MalformedInlineRemoteTarget`
- `CrossHostDelivery::deliver_remote` must not use a generic outer error type;
  the outer `Err` arm is reserved for unclassified infrastructure failures and
  uses one dedicated typed error surface (`CrossHostDeliveryInfraError`)

Dispatch rules:

- empty normalized remote-host field => local mailbox path
- non-empty normalized remote-host field => cross-host delivery boundary
- sender-side daemons must not write a remote target directly into a local
  mailbox path
- `localhost` and the sender host's own advertised or bound IP address are
  ordinary non-empty remote-host values on that same cross-host delivery path
- no localhost-only or loopback-only dispatch branch exists in the production
  contract

Delivery-result rules:

- if the cross-host path is healthy, the CLI may wait up to `10s` for remote
  daemon acceptance
- if the cross-host path is unhealthy, the CLI returns immediately with a
  deferred-delivery result
- healthy means the daemon has a currently usable enabled interface row, a
  resolvable outbound target, and no cached terminal-failure state for that
  host
- deferred retry is allowed only for transient failure kinds:
  - connect timeout
  - connection refused
  - connection reset
  - host/network unreachable
- terminal failure kinds never spend the retry budget:
  - allowlist rejection
  - authentication / certificate rejection
  - protocol rejection
  - malformed target
- the daemon may continue bounded background retry for `60s..120s` using:
  - initial retry interval: `5s`
  - exponential backoff factor: `2x`
  - maximum interval cap: `30s`
  - bounded jitter: `±20%`
  - hard attempt cap: `6` attempts per deferred delivery
- the daemon emits one final delivery/failure receipt into the sender inbox at
  retry completion or terminal failure
- deferred-delivery state is durable for the bounded retry window: if the
  daemon restarts mid-window, the pending retry/receipt obligation is resumed
  from durable state until the window expires, then one final receipt is still
  emitted
- deferred background work is bounded to `256` concurrent remote deliveries per
  host; additional deferred candidates fail fast with a typed overload result
  rather than creating unbounded in-memory growth
- `RemoteFailureKind` is the concrete type-level encoding of the phase plan's
  authoritative `Transient` / `Terminal` runtime retry-decision taxonomy

Boundary rule:

- parsing/normalization/classification ends at one typed remote-target
  contract
- ATM payload framing and message shapes remain transport-invariant across
  local IPC, socket transport, and any other supported ATM transport
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
