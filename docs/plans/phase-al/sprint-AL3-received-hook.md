# AL.3 — Post-Persistence Received Hook

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.2 and the AK.11 hook-contract gate.
**unblocks:** AL.5.
**parallel_safe:** AL.4, because it owns client/connectors and has no hook or
shared-dispatch changes.

## Deliverables

1. Invoke the injected `MessageReceivedHookEmitter` only from the one
   successful, newly-persisted inbound write result.
2. Retain the existing tmux implementation as an injected composition choice.
   Graft remains an independently-started client/receiver implementation; the
   runtime and daemon have no graft crate dependency.
3. Map hook failure to retained warning information without changing the
   successful receive/write result.

```rust
match persisted_write.is_newly_persisted() {
    true => emit_message_received_warning_only(&state.hook, persisted_write),
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

## Non-closure

This sprint does not add a new tmux/graft transport or modify their receiver
UX. It only preserves the AK.11 injection boundary.
