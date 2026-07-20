---
title: AI.5 chat address identity
status: proposed
branch: feature/pAI-s5-chat-address-identity
worktree: ../atm-core-worktrees/feature/pAI-s5-chat-address-identity
target: integrate/phase-AI
---

# AI.5 — chat address identity

## Deliverables

1. Introduce ADR-037's optional `ChatId` and canonical `AgentAddress` model:
   `<agent>[:<chat-id>]@<team>[.<host>]`.
   Cherry-pick Phase AG commit `924861da` into the Phase AI integration line
   before this work and reuse its central `atm_storage::validate_path_segment`
   for agent, team, and chat-id validation.
2. Persist nullable source/destination chat-id columns independently from agent
   names, migrate existing rows as null, and preserve both fields in canonical
   message projections.
3. Make read `from`, write `to`, nudge display, reply construction, inbox
   visibility, owner-only mutation, and acknowledgement targeting use the same
   full address. Agent-facing rendering concatenates a present value as
   `agent:chat-id`.
4. Add message search semantics: `--agent <agent>` spans all chat IDs and
   `--agent <agent> --chat <chat-id>` narrows to one identity. Finalize the
   matching structured address/filter schema in OpenAPI; do not create a
   session header or a separate chat delivery path.
5. Add structural tests rejecting chat-id parsing/rendering outside the
   canonical address type.

## Acceptance criteria

- `hendrix@hermes`, `hendrix:12345@hermes`, and
  `hendrix:98765@hermes` are distinct identities for visibility and mutation.
- A reply or acknowledgement targets the original full address, including its
  chat-id; no chat-specific send/ack/nudge route exists.
- Nullable source/destination columns are separate from agent-name columns;
  existing rows remain readable.
- The API, CLI, graft, Python-facing projection, and nudge render the same
  chat-qualified address.
- `--agent hendrix` finds all of Hendrix's messages regardless of chat ID;
  `--agent hendrix --chat 12345` finds only `hendrix:12345`.
- One validator rejects invalid agent, team, and chat-id segments; no adapter
  owns a second identifier character policy.

## Required validation

Address parser negatives; migration tests; chat-separated inbox/read/ack/reply
integration tests; OpenAPI schema tests; `just lint`; `just test`.
