---
id: AI.17
title: Ambient Chat Context (Hermes First Client)
status: complete
branch: feature/pAI-s17-hermes-chat-identity
worktree: ../atm-core-worktrees/feature/pAI-s17-hermes-chat-identity
target: integrate/phase-AI
---

# Sprint AI.17 — Ambient Chat Context (Hermes First Client)

## Goal

Implement the missing environment caller-context forms on Phase AI’s existing
chat identity contract. Hermes is the first client to use them. This sprint
adds no schema migration, no new CLI flag, and no daemon protocol.

`ATM_CHAT_ID=<id>` is a client-neutral optional `chat_id` for the ambient
`ATM_IDENTITY`; it is rendered to agents as `agent:<id>@team`. Hermes is the
first adapter to consume it. It has the same meaning as `--chat-id <id>` and
`--as <agent>:<id>` under ADR-037.

## Hard Dependencies

- AI.5's accepted chat-address contract is present on `integrate/phase-AI`;
  entry evidence names that commit and green `just lint` / `just test` results.
- ADR-037 and the Phase AI CLI/API tests are present on that baseline.
- ADR-039 Python graft host binding governs the Hermes-side use of this
  mapping; AI.17 may not create a second host boundary.

## Deliverables

- One caller-context resolver applying: `--as`, then `--chat-id`, then
  `ATM_CHAT_ID`, then qualified `ATM_IDENTITY`, then no chat-id. An
  unqualified `--as` explicitly selects no chat-id.
- Updated `atm help identity` and identity documentation describing
  `ATM_CHAT_ID`, qualified `ATM_IDENTITY`, and that precedence.
- Focused tests proving:
  - each precedence level wins over every lower level;
  - missing values produce no chat ID;
  - malformed or delimiter-containing environment values fail before a daemon
    request;
  - qualified `ATM_IDENTITY` and `ATM_CHAT_ID` preserve distinct chat IDs;
  - the mapping agrees with Phase AI’s `--chat-id` and `--as` equivalence.

## Contract

```rust
pub fn resolve_caller_chat_id(
    explicit_as: Option<&AgentIdentity>,
    explicit_chat_id: Option<&ChatId>,
    ambient_chat_id: Option<&str>,
    ambient_identity: &AgentIdentity,
) -> Result<Option<ChatId>, AtmError>;
```

`resolve_caller_chat_id` is the only AI.17–AI.21 code allowed to interpret
`ATM_CHAT_ID`. It delegates segment validation to the Phase AI address type;
an empty key maps to `None`, while an invalid non-empty key is a typed
configuration error.

## Boundary and Non-Goals

The resolver consumes typed caller context; it does not parse display strings,
construct SQL, create a `session_id`, or add `--session` / `--session-id`.
It cannot change the Phase AI parser, HTTP API, canonical write path, or
post-write router.

## Parallel Execution

AI.17 may run in parallel with AI.11–AI.16 because it does not modify
`atm-graft`, daemon transport, HTTP, storage, or a shared Phase AI contract.
It must rebase and be re-reviewed if the consumed chat-address contract changes.

## Closure

- All deliverables and focused tests pass.
- `crates/atm/src/commands/help.rs` and
  `docs/user-documents/identity-and-team.md` describe the implemented
  precedence and use `ATM_CHAT_ID`; their source/help tests pass.
- `just lint`, `just test`, and `git diff --check` pass.
- The completion report identifies the mapping function and tests by file and
  symbol, plus the exact Phase AI baseline commit.
