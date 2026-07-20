# ADR-028B — Protocol Closed to Three Verbs

| Field | Value |
| --- | --- |
| ID | ADR-028B |
| Status | **Accepted** |
| Date | 2026-07-20 |
| Deciders | Rand Lee |
| Relates to | ADR-028A (error consolidation), ADR-028C (cross-host implementation) |
| Supersedes | ADR-027 (compatibility becomes informational, not fail-closed) |

## Decision

The wire protocol is exactly three verbs:

- **read** — subsumes List, Peek, Receive, Doctor (parameterized queries, not
  separate message kinds)
- **write** — subsumes Send, Clear (mutations)
- **nudge** — the existing wake/delivery signal (its role is expanding under
  ADR-028C; the name itself is flagged there as under review, not decided
  here)

`ResponseEnvelope` collapses to match: `ReadResult | WriteResult | NudgeAck |
Error` — four variants, down from nine.

Enforcement is mechanical, not documentary: a snapshot test asserting the
closed set exactly —

```rust
assert_eq!(MessageKind::ALL, [Read, Write, Nudge]);
```

— so a fourth variant fails CI immediately and unambiguously, regardless of
whether the session adding it has any memory of this ADR.

## Punchlist — every `MessageKind` variant found, and its disposition

`MessageKind` currently has ~19 variants (request + response combined,
`atm-core/src/protocol.rs`, 1,283 lines).

| Variant | Verified status | Disposition |
| --- | --- | --- |
| `Send` | Live, production | Maps to **write** |
| `Ack` (`SendAcknowledgeRequest`) | Reported deleted by decider; still present on the cloned `main` branch as of this review | Maps to **write** once landed. **Discrepancy noted below.** |
| `Clear` | Live, production | Maps to **write** |
| `List` | Live, production | Maps to **read** |
| `Peek` | Live, production | Maps to **read** |
| `Receive` | Live, production | Maps to **read** |
| `Doctor` | Live, production | Maps to **read** |
| `Heartbeat` (`TeamMemberHeartbeatRequest`/`Response`) | Verified: every non-test construction of `RequestEnvelope::Heartbeat` is inside `#[cfg(test)]`. Only non-test references are dispatch routing and replay-metadata keying — nothing production ever sends one. Feeds `RuntimeStatusCache` → `RuntimeStatusSnapshot` *if* received. | **Deleted.** Built for a future feature, never wired to a caller. |
| `CompatibilityPreflight` / `CompatibilityVerdict` | Verified live in production: `atm-graft/transport.rs`, `atm/src/composition.rs`, `atm-daemon-client/compatibility.rs`. `CompatibilityVerdict::Incompatible` returns `ClientDaemonVersionIncompatible` and hard-fails the connection on any version mismatch. | **Deleted** as a connection-blocking handshake. Pre-launch, this turns "rebuilt one binary" into "nothing connects," which the decider had to manually disable to keep working. Version stays available for diagnostics; it never blocks a request. Supersedes ADR-027's `Connection<Unverified>`/`Connection<VersionVerified>` typestate machine. |

| `ResponseEnvelope::Send` | Live | Folds into **WriteResult** |
| `ResponseEnvelope::CompatibilityVerdict` | Live (see above) | **Deleted** with the request variant |
| `ResponseEnvelope::Heartbeat` | Test-only (see above) | **Deleted** with the request variant |
| `ResponseEnvelope::List` / `Peek` / `Receive` / `Doctor` | Live | Fold into **ReadResult** |
| `ResponseEnvelope::Clear` | Live | Folds into **WriteResult** |
| `ResponseEnvelope::Error` | Live | Retained as **Error** (see ADR-028A for its shape) |

**Discrepancy to resolve before merge:** the decider stated `Ack` is already
deleted; the cloned repository (GitHub, current `main`) still shows
`SendAcknowledgeRequest` as a live variant. Either the deletion hasn't been
pushed, or it's sitting in a session that hasn't landed. Verify actual branch
state before treating `Ack` as gone.

## Consequences

- `MessageKind` goes from ~19 variants to 3; `ResponseEnvelope` from 9 to 4.
- Every dispatch site's `match` becomes exhaustive over a much smaller,
  closed set — Rust's exhaustiveness check means a future addition breaks
  every dispatch site until handled, though (per ADR-028C's discussion of
  enforcement) that alone doesn't stop an agent from "handling" it by adding
  a branch — the snapshot test is what actually blocks the addition itself.
- Connection setup no longer has a version-mismatch failure mode blocking
  work pre-launch.

## Alternatives considered

- **Keep `Heartbeat` for future use.** Rejected: dead code with no caller is
  a false signal of capability; re-add it when an actual caller exists.
- **Keep `CompatibilityPreflight` but make `Incompatible` non-fatal.**
  Considered. Rejected in favor of full deletion — pre-launch there is no
  version-skew scenario to protect against yet, and a half-kept check invites
  the same "why does this exist" question later without a clear answer.
