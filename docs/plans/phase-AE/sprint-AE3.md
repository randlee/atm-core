---
id: AE.3
title: pi-agent-atm And codex-atm atm-graft Harness Support
status: planned
branch: feature/pAE-s3-atm-graft-harnesses
worktree: ../atm-core-worktrees/feature/pAE-s3-atm-graft-harnesses
target: integrate/phase-AE
---

# Sprint AE.3 — pi-agent-atm And codex-atm atm-graft Harness Support

## Goal

- register pi-agent-atm and codex-atm as supported atm-graft harness implementations
  under the existing `PostSendHookEmitter` trait from Phase AD.6-8

## Hard Dependencies

- `AE.2` complete
- `docs/plans/phase-AE/plan-phase-AE.md`
- Phase AD.6-8 post-send emitter contract
- `#461`
- pi-agent-atm running end-to-end (externally validated)

## Exact Targets

- `crates/atm-graft/src/`
- `crates/atm-core/src/post_send/` (emitter registry)
- `crates/atm-daemon/src/post_send/` (emitter dispatch)
- `docs/atm-graft/`
- `docs/atm-core/boundaries.md` (if new boundary added)

## Harness Registration

Each harness is an implementation of the `PostSendHookEmitter` trait:

```rust
pub trait PostSendHookEmitter: sealed::Sealed {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
```

### pi-agent-atm emitter

- transport: Unix domain socket (atm-graft protocol)
- sends `PostSendHookEvent` as JSON over the graft socket
- pi-agent-atm process receives and handles the nudge (context injection or notification)
- registration: daemon discovers pi-agent-atm via graft socket handshake
- failure: `emit()` returns `Err(AtmError::GraftEmitFailed { harness, details })`

### codex-atm emitter

- transport: Unix domain socket (atm-graft protocol), same contract
- sends `PostSendHookEvent` as JSON over the graft socket
- codex-atm process receives and handles the nudge
- registration: same discovery mechanism as pi-agent-atm
- failure: same error contract

## Emitter Discovery

The daemon maintains an emitter registry keyed by agent identity:

```
agent "pi-agent-atm" → GraftEmitter { socket_path: "/tmp/atm-graft-pi-agent.sock" }
agent "codex-atm"     → GraftEmitter { socket_path: "/tmp/atm-graft-codex.sock" }
```

Discovery flow:
1. Daemon scans known graft socket paths on startup
2. Each socket performs handshake → returns agent identity + capabilities
3. Registered emitters are available for post-send dispatch
4. Agents that expose `post_send_hook` capability get their emitter invoked on send

## Deliverables

- pi-agent-atm registered as an atm-graft harness that receives post-send nudges
- codex-atm registered as an atm-graft harness that receives post-send nudges
- `atm send team-lead "test"` when team-lead runs in pi-agent-atm → nudge delivered via graft
- failed graft emission produces sender-visible warning (per AD.6-8 contract)
- emitter discovery works on daemon startup

## Required Validation

- pi-agent-atm receives post-send event via graft socket after `atm send`
- codex-atm receives post-send event via graft socket after `atm send`
- graft socket handshake succeeds on daemon startup
- failed graft emission logs error and returns sender warning
- unregistered agent does not attempt graft emission
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
