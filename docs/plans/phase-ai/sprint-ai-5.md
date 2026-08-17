---
title: AI.5 chat address identity
status: complete
branch: feature/pAI-s5-chat-address-identity
worktree: ../atm-core-worktrees/feature/pAI-s5-chat-address-identity
target: integrate/phase-AI
---

# AI.5 — chat address identity

## Deliverables

1. Introduce ADR-037's optional `ChatId` and canonical `AgentAddress` model:
   `<agent>[:<chat-id>]@<team>[.<host>]`.
   Reuse AI.1's retained `atm_storage::validate_path_segment` contract for
   agent, team, and chat-id validation; do not create an adapter-local policy.
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
6. Add the planned send conveniences without adding a second address model:
   `atm send <to> --chat-id <chat-id> <message>` resolves the caller as the
   ambient `ATM_IDENTITY` plus that chat-id, exactly as `atm send <to> --as
   <agent>:<chat-id> <message>` does. `--chat-id` and `--as` are mutually
   exclusive. A chat-qualified recipient remains canonical
   `<agent>:<chat-id>@<team>[.<host>]`. The same equivalence applies to `atm read`: with
   `ATM_IDENTITY=omega-prime`, `atm read --chat-id 1234` reads as
   `omega-prime:1234`, equivalently `atm read --as omega-prime:1234`.
   The caller team still comes from `--team` or `ATM_TEAM`.
   `--from` remains a read/list sender filter and is never a caller override.

## Contract

```rust
pub struct ChatId(/* validated safe segment */);

pub struct AgentIdentity {
    pub agent: AgentName,
    pub chat_id: Option<ChatId>,
}

pub struct AgentAddress {
    pub identity: AgentIdentity,
    pub team: TeamName,
    pub host: Option<HostName>,
}

pub struct MessageParticipantFilter {
    pub agent: AgentName,
    pub chat_id: Option<ChatId>,
    pub direction: ParticipantDirection,
}

pub enum ParticipantDirection { From, To, Either }
```

`AgentIdentity::from_str` is the sole parser for `agent[:chat]`: thus
`agent:XXX` always means agent `agent` with `chat_id=Some(XXX)`. It is used by
`--as`, the `--chat-id` caller shorthand, and full addresses.
`AgentAddress::from_str` is the sole full-address parser for
`agent[:chat]@team[.host]`; it splits once at `@`, delegates the left component
to `AgentIdentity`, then splits the right component at its first `.`. A team
has no `.`, so a DNS or IP host may contain further periods. The inherited
segment validator remains the shared leaf validation policy; no CLI, graft,
nudge, or transport adapter may split these delimiters. A future Phase AH
Python binding uses these parsers unchanged.

CLI caller resolution composes the same components: base agent plus `--team`
is the logical `agent@team`; adding `--chat-id XXX` is the logical
`agent:XXX@team`. It normalizes to `AgentAddress` once before dispatch.
`impl Display for AgentAddress` is the sole agent-facing renderer; callers
must not concatenate agent, chat, team, or host fields themselves.

The two caller spellings normalize to the same `AgentAddress` before daemon
dispatch. `--chat-id` augments the ambient caller identity, not the recipient;
`--as` is the explicit equivalent caller-context override. Neither creates a
second wire or storage field.

## Acceptance criteria

- `hendrix@hermes`, `hendrix:12345@hermes`, and
  `hendrix:98765@hermes` are distinct identities for visibility and mutation.
- A reply or acknowledgement targets the original full address, including its
  chat-id; no chat-specific send/ack/nudge route exists.
- Nullable source/destination columns are separate from agent-name columns;
  existing rows remain readable.
- The API, CLI, graft, and nudge render the same
  chat-qualified address.
- `--agent hendrix` finds all of Hendrix's messages regardless of chat ID;
  `--agent hendrix --chat 12345` finds only `hendrix:12345`.
- with `ATM_IDENTITY=omega-prime`, `atm send <to> --chat-id 1234 <message>`
  and `atm send <to> --as omega-prime:1234 <message>` produce the same
  caller address; passing both flags fails before daemon dispatch.
- with the same environment, `atm read --chat-id 1234` and `atm read --as
  omega-prime:1234` select the same owner mailbox; passing both flags fails
  before daemon dispatch.
- a recipient chat-id is expressed only in the canonical `<to>` address;
  `--from` cannot override a sender on `send`.
- One validator rejects invalid agent, team, and chat-id segments; no adapter
  owns a second identifier character policy.

## Required validation

Address and caller-resolution tests must cover:

- `AgentIdentity::from_str("omega-prime")` produces no chat-id, while
  `AgentIdentity::from_str("omega-prime:1234")` produces
  `chat_id=Some("1234")`; malformed, empty, and delimiter-containing segments
  fail before dispatch;
- `AgentAddress::from_str("omega-prime:1234@atm-dev")` equals the structured
  address composed from identity `omega-prime:1234` and team `atm-dev`;
- with `ATM_IDENTITY=omega-prime` and `ATM_TEAM=atm-dev`, `atm send <to>
  --chat-id 1234 <message>` and `atm send <to> --as omega-prime:1234
  <message>` construct equal `WriteRequest.caller` values and use the same
  write handler;
- under the same environment, `atm read --chat-id 1234` and `atm read --as
  omega-prime:1234` resolve the same owner address and execute the existing
  owner-only read path;
- combining `--as` and `--chat-id`, using `--from` as a send override, or
  using an `--as` base agent different from `ATM_IDENTITY` fails before daemon
  dispatch;
- CLI, graft, and HTTP parsing of the same structured `AgentAddress` reach
  the same canonical write/read handlers without chat-specific branches.

Also run migration tests; chat-separated inbox/read/ack/reply integration
tests; OpenAPI schema tests; `just lint`; and `just test`.
