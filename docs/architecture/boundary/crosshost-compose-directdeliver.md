# Transport And IPC Boundary Guidelines

This document defines stable transport and IPC rules.

## Transport rule

Transport moves bytes and returns transport facts.

Transport does not own:

- routing policy
- mailbox mutation
- retry policy
- deferred / terminal / unknown classification
- sender receipt policy
- ack semantics
- endpoint configuration policy

## Single routing rule

The local vs cross-host decision is made once, above transport.

Downstream transport code must consume a resolved target or resolved endpoint.
It must not re-parse, re-match, or re-classify from raw request fields.

## Single send path rule

All outbound message delivery uses one canonical send path.

- send and ack share the same outbound path
- ack is message data, not a second transport workflow
- loopback, localhost, self-IP, same-host IP, and cross-host all use the same
  production route

## Single receive path rule

All inbound messages use one receive-side persistence and nudge path.

Transport source does not change receive semantics.

The existing peer receiver may decode one bounded `messages[]` body on the
same write route. It validates every member before one atomic persistence
operation, then uses this ordinary receive-side post-write path for each
committed member. Post-commit nudge/notification errors are warnings, not
receive failures or sender receipts.

## Wire contract rule

The wire contract is shared across:

- local IPC
- cross-host sockets
- graft integration
- future HTTP replacement

Do not create transport-specific message semantics.

## What to flag

Flag a boundary violation when any of these appear:

- two top-level send pipelines
- separate transport path for ack
- local-vs-remote decision below the routing boundary
- transport-level mutation of canonical request meaning
- endpoint parsing/config lookup in transport
- transport-owned replay or retry orchestration
- transport-owned sender receipt synthesis
- test-only transport branch preserved in production

## What to prefer

- one send-shaped request contract
- one send-shaped response contract
- one routing boundary
- one endpoint-resolution boundary
- one higher-level policy owner above transport
