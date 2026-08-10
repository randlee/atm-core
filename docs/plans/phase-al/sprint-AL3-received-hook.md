---
title: AL.3 Post-Persistence Received Hook
status: complete
branch: feature/pal-s3-received-hook
worktree: ../atm-core-worktrees/feature/pal-s3-received-hook
target: integrate/phase-al
---

# AL.3 — Post-Persistence Received Hook

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.2 and AL.1's archived AK.11 exact-copy hook gate. AL.2's
pushed integration commit must be merged forward before each development/fix
round; AL.2 PR merge is not required.
**unblocks:** AL.8.
**parallel_safe:** AL.4, because it owns client/connectors and has no hook or
shared-dispatch changes.

**traceability:** archived AK.11 hook contract, `REQ-CORE-TRANSPORT-002/004`,
ADR-033, ADR-036. The unchanged warning representation identified by AL.1 is
required; this sprint may not add a public field to express it.

## Deliverables

1. Invoke the injected `MessageReceivedHookEmitter` only from the one
   successful, newly-persisted inbound write result.
2. Retain the existing tmux implementation as an injected composition choice.
   Graft remains an independently-started client/receiver implementation; the
   runtime and daemon have no graft crate dependency.
3. Map hook failure to retained warning information without changing the
   existing successful receive/write schema. If current schema cannot carry
   that warning, stop and request an API-contract decision rather than add a
   new result type or field.
4. Record the reviewed ADR-041 interpretation for bounded in-request hook
   execution before AM.5 can delete the legacy behavior; it must state the
   sender-observed latency and warning contract rather than deferring it.

```rust
match persisted_write.is_newly_persisted() {
    true => record_received_hook_warning_after_persistence(&state.hook, persisted_write),
    false => None,
}
```

The exact persistence-result API may differ, but it must convey the
new-versus-idempotent distinction explicitly; a payload mutation or sender-side
condition is not allowed.

## Acceptance criteria

- One newly persisted HTTP write produces exactly one hook invocation.
- A duplicate message ID is accepted/logged as idempotent and produces no hook.
- An injected hook error yields the ordinary successful response plus warning
  diagnostics; it does not return an HTTP receive failure.
- No sender/client code calls a notification hook.

## Required validation

- deterministic recording-emitter integration tests for all three criteria
- test proving UDS/TCP/peer provenance fixtures reach the same hook call site
- architecture test forbidding `MessageReceivedHookEmitter` from client code
- cancellation/deadline test proving no detached hook thread/task, queue, or
  sender-side retry is created

## Non-closure

This sprint does not add a new tmux/graft transport or modify their receiver
UX. It only preserves the AK.11 injection boundary.
