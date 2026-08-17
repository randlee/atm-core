# ADR-031 — Remote-Target Contract And Cross-Host Dispatch Boundary

| Field | Value |
| --- | --- |
| ID | ADR-031 |
| Status | Superseded by ADR-034 and ADR-035 |
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

## Historical proposal (retired)

Phase AG proposed exactly two operator-facing remote-send forms:

`atm send <agent>@<team>.<host> ...`

Host qualification is part of the one typed destination-address grammar; no
second `--host` input or alternate route exists.

Illustrative contract types:

```rust
pub struct RemoteTargetHost(String);

pub enum SendTargetParseError {
    MissingTeam,
    MissingHost,
    InvalidAgentNameDot,
    InvalidTeamNameDot,
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

- agent/member names and team names are path-segment-like identifiers rather
  than free-form labels
- the only allowed characters in agent/member names and team names are ASCII
  letters, ASCII digits, `-`, and `_`
- agent/member names and team names must reject:
  - path delimiters: `/` and `\`
  - traversal forms: `.` and `..`
  - reserved address delimiters: `.` and `:`
  - whitespace
  - wildcard or pattern characters that could be interpreted by current or
    future parsers, including at minimum `*`, `?`, `[` and `]`
- inline parsing splits at the first `.` after `@`; team names cannot contain
  `.`, while a DNS or IP host may contain later periods
- each parser rejection maps to one stable error code / variant:
  - missing team => `MissingTeam`
  - missing host => `MissingHost`
  - reserved or unsupported agent/member charset => stable typed validation
    failure on the agent segment
  - reserved or unsupported team charset => stable typed validation failure on
    the team segment
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

## Supersession

The retry, durable deferred-delivery, receipt, and per-host state described
above are not part of the approved design. ADR-034 and ADR-035 replace this
decision with a transport-only HTTPS adapter and one canonical write path.

## Historical consequences

- AG.11 owns the typed remote-target contract and production wiring proof
- AG.12 and AG.13 prove that localhost and self-IP use the ordinary remote-host
  path
- AG.14 must include at least one ADR-003 Tier-3 regression that exercises the
  production cross-host delivery path end to end
- current host-routing policy is ADR-035 and `REQ-CORE-TRANSPORT-002`; this
  retired proposal is not authority for new work
