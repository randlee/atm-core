# ATM-Core Crate Architecture

## 1. Purpose

This document defines the `atm-core` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns crate-local structure and
service boundaries.

## 2. Architectural Rules

- `atm-core` exposes request/result/service boundaries, not clap surfaces.
- `atm-core` owns workflow/state transitions and must enforce them by code
  structure.
- `atm-core` owns observability as an injected boundary, not as a concrete
  dependency on `sc-observability`.
- `atm-core` must keep mailbox/config/workflow/log/doctor logic reusable across
  CLI contexts.

## 3. ADR Namespace

The `atm-core` crate uses the `ADR-CORE-*` namespace.

Initial use cases:

- typestate and workflow decisions
- mailbox boundary decisions
- config/loading decisions
- observability port decisions
- service/module boundary decisions

## 4. Post-Send Hook Design

### 4.1 Config Shape

The post-send hook extends `.atm.toml` with a per-agent table:

```toml
identity = "team-lead"
default_team = "atm-dev"

[agents.arch-ctm]
post_send = "tmux send-keys -t '$PANE' -l 'atm read --team atm-dev' && tmux send-keys -t '$PANE' Enter"
```

`atm-core` should represent this with recipient-scoped config types:

```rust
use std::collections::HashMap;

pub struct AtmConfig {
    pub identity: Option<String>,
    pub default_team: Option<String>,
    pub agents: HashMap<String, AgentConfig>,
}

pub struct AgentConfig {
    pub post_send: Option<String>,
}
```

This keeps the hook configuration in `atm-core::config` rather than leaking it
into CLI-only code. The send path reads the hook from the resolved recipient's
agent entry, not from the sender identity.

### 4.2 Send-Path Invocation Point

The hook belongs in `crates/atm-core/src/send/mod.rs` immediately after the
message write succeeds. In today's code, that is the point immediately after
`mailbox::append_message(&inbox_path, &envelope)?;`. In the planned refactor,
the same rule is expressed as "after `WriteOutcome::Success`".

This placement preserves the required ordering:

1. Resolve sender, recipient, and message body.
2. Validate the target team/member.
3. Persist the mailbox message.
4. Spawn the optional hook as a best-effort side effect.
5. Return the successful send outcome.

Dry-run sends skip the mailbox write and therefore skip the hook.

### 4.3 Spawn Pattern And Error Boundary

The hook is a fire-and-forget subprocess launched with `sh -c`:

```rust
std::process::Command::new("sh")
    .arg("-c")
    .arg(hook)
    .env("ATM_SENDER", &sender)
    .env("ATM_RECIPIENT", &recipient.agent)
    .env("ATM_MESSAGE_BODY", &body)
    .spawn()
```

The hook environment is intentionally self-describing:

- `ATM_SENDER` identifies who sent the message
- `ATM_RECIPIENT` identifies which agent's hook is being triggered
- `ATM_MESSAGE_BODY` carries the full persisted message text so helper scripts
  can extract embedded task metadata

Spawn failures stay on the best-effort side of the boundary. The send service
must preserve the successful mailbox write result even when the hook command is
missing, exits immediately, or cannot be spawned.
