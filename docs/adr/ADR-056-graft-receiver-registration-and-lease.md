# ADR-056 — Graft Receiver Registration And Lease Semantics

Status: accepted

Date: 2026-08-26

## Context

Graft receivers bind same-host loopback listeners and historically published
their endpoint and capability in a JSON record. A truncating-write race in
that record can make a live receiver unreachable, and the daemon and receiver
must be able to restart independently without a manual profile reset.

## Decision

The replacement daemon owns a durable SQLite registry of graft receiver
leases. A receiver registers `(team, agent, endpoint, capability,
owner_generation)` after acquiring its same-host ownership flock. The registry
stores registration and refresh timestamps plus advisory `unreachable_at`
feedback. Capabilities are encoded with `LocalCapability::to_base64url()` on
the wire and at rest, and decoded with `parse_base64url()`.

The natural key is `(team, agent)`. It is intentionally a join key rather than
a SQL foreign key to `team_roster`: roster saves replace all rows for a team,
so a foreign key would either delete live leases during normal roster refresh
or reject the roster write.

`register` unconditionally upserts and displaces a row whose owner generation
differs. The flock is the same-host exclusivity mechanism and is acquired
before registration; the lease table is not a write-time conflict gate.
Matching generations refresh the lease. `refresh`, `unregister`, and
`mark_unreachable` require the stored owner generation and return `NotOwner`
on mismatch. `AlreadyActive` remains reserved for a future caller that cannot
prove same-host exclusivity. Delivery-time unreachable feedback never deletes
the row.

Liveness is derived by readers from lease existence, refresh age, and
`unreachable_at`; no status boolean is persisted. The two writer families are
the receiver lifecycle and delivery feedback. Lookup is local-ingress-only,
read-only, and returns `Ok(None)` for a missing row.

## Consequences

- SQLite persistence survives daemon restart and makes receiver/daemon
  lifecycle independence explicit.
- The replacement router validates local ingress, roster membership, and
  loopback endpoint addresses before invoking the storage boundary.
- AQ1.6 owns receiver-side announce/refresh/unregister calls and AQ1.7 owns
  production bootstrap wiring and consumer cutover.
- The JSON record remains dual-written until AQ1.8; this ADR removes its role
  as the daemon's source of truth but does not change file-record behavior in
  AQ1.5.

## Supersession

This decision supersedes the endpoint-publication portion of
`AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE`. Its flock and generation
checked ownership fixes remain valid; AQ1.8 removes the remaining file-record
read/write machinery after the registration and consumer sprints land.
