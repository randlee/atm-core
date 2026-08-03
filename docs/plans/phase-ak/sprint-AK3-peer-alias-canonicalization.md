---
title: AK.3 Canonical peer alias persistence
status: proposed
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.2
parallel_safe: false
---

# AK.3 — canonical peer alias persistence

## Closure

Normalize configured peer aliases before persistence. This changes only the
stored destination representation; AK.3 adds no outbound delivery.

## Fixed contract

```rust
struct PeerEndpoint {
    host: HostName,
    port: NonZeroU16,
}

fn normalize_peer_alias(alias: &HostName) -> Result<PeerEndpoint, AtmError>;
```

The configuration-owned alias index makes an O(1) local substitution: `m5`,
`rand-m5.local`, and an explicit configured IP alias can each normalize to
`rand-m5.local`. SQLite stores only that full hostname, never a resolved IP.
A configured host is admitted offline; no live DNS, peer scan, or thread is
needed for normalization.

## Deliverables

1. Add the configuration-owned alias index and canonical full-host endpoint
   type. Keep IP aliases explicit configuration, not inferred DNS results.
2. Normalize every host-qualified recipient before canonical persistence,
   including the destination host retained in the origin record's immutable
   `peerOutbound` delivery record.
3. Preserve the full hostname through send, mailbox, ACK, and nudge data;
   preserve the current no-delivery behavior from AK.2.
4. Update peer configuration requirements and schema/boundary documentation.

## Required validation

- Unit: aliases for one peer normalize to one full hostname without SQLite,
  live DNS, a peer scan, or a new thread.
- Unit: unknown alias fails before persistence; configured unreachable peer
  persists the canonical hostname while remaining undelivered.
- `just lint` and `just test` pass.

## Dependencies

Before every AK.3 development/fix round, merge AK.2 into AK.3. Start AK.3 as
soon as AK.2 is pushed; do not wait for QA. AK.3 PR completion waits for AK.2
merge. Push AK.3, then start AK.4 with AK.3→AK.4 merge-forward.
`must_follow` is required because AK.4 sends AK.3's persisted canonical host;
it is not parallel-safe because both change host-qualified admission/routing.
