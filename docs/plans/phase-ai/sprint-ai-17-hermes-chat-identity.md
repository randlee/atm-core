---
id: AI.17
title: Hermes Chat Identity Mapping
status: planned
branch: feature/pAI-s17-hermes-chat-identity
worktree: ../atm-core-worktrees/feature/pAI-s17-hermes-chat-identity
target: integrate/phase-AI
---

# Sprint AI.17 — Hermes Chat Identity Mapping

## Goal

Prove and implement the narrow Hermes mapping onto Phase AI’s existing chat
identity contract. This sprint adds no ATM schema migration, no CLI option,
and no new daemon protocol.

`HERMES_SESSION_KEY=<id>` becomes the optional `chat_id` of the ambient
`ATM_IDENTITY`; it is rendered to agents as `agent:<id>@team`. It has the
same meaning as `--chat-id <id>` and `--as <agent>:<id>` under ADR-037.

## Hard Dependencies

- AI.5's accepted chat-address contract is present on `integrate/phase-AI`;
  entry evidence names that commit and green `just lint` / `just test` results.
- ADR-037 and the Phase AI CLI/API tests are present on that baseline.
- ADR-039 Python graft host binding governs the Hermes-side use of this
  mapping; AI.17 may not create a second host boundary.

## Deliverables

- A single Hermes adapter function that validates and maps
  `HERMES_SESSION_KEY` into `Option<ChatId>` for the existing typed caller
  address.
- A documented deterministic Hermes chat key derived from the complete
  canonical source address: `atm:<agent>[:<chat-id>]@<team>`.
- Focused tests proving:
  - `omega-prime` with key `1234` is `omega-prime:1234`;
  - missing key produces no chat ID;
  - malformed or delimiter-containing keys fail before a daemon request;
  - two chat IDs for one agent map to distinct Hermes chats;
  - the mapping agrees with Phase AI’s `--chat-id` and `--as` equivalence.

## Contract

```rust
pub fn hermes_chat_id(session_key: Option<&str>) -> Result<Option<ChatId>, AtmError>;
pub fn hermes_chat_key(source: &AgentAddress) -> String;
// "atm:<agent>[:<chat-id>]@<team>"
```

`hermes_chat_id` is the only AI.17–AI.21 code allowed to interpret
`HERMES_SESSION_KEY`. It delegates segment validation to the Phase AI address
type; an empty key maps to `None`, while an invalid non-empty key is a typed
configuration error. `hermes_chat_key` consumes the typed address and never
parses rendered display text.

## Boundary and Non-Goals

The adapter consumes `AgentAddress`; it does not parse display strings,
construct SQL, create a `session_id`, or add `--session` / `--session-id`.
It cannot change the Phase AI parser, HTTP API, canonical write path, or
post-write router.

## Parallel Execution

AI.17 may run in parallel with AI.11–AI.16 because it does not modify
`atm-graft`, daemon transport, HTTP, storage, or a shared Phase AI contract.
It must rebase and be re-reviewed if the consumed chat-address contract changes.

## Closure

- All deliverables and focused tests pass.
- `just lint`, `just test`, and `git diff --check` pass.
- The completion report identifies the mapping function and tests by file and
  symbol, plus the exact Phase AI baseline commit.
