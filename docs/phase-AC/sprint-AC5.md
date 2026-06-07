# AC.5 RPC Envelope And Domain Type Unification

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.5
worktree: ../atm-core-worktrees/feature/pAC-s5-rpc-envelope-and-domain-type-unification
branch: feature/pAC-s5-rpc-envelope-and-domain-type-unification
status: planned
estimated_scope: large
```

## Goal

Replace per-message transport DTO proliferation with one generic RPC envelope
that carries canonical domain bodies.

## Scope Summary

This sprint is the RPC/domain reset line. It does not redefine the storage
contract. It makes the transport layer consume the same canonical structs that
the storage layer now uses.

Production-ready commitment:
- every deliverable listed in this sprint is expected to land at a
  production-ready level for the RPC/body convergence scope this sprint claims;
  leaving the real traffic on old transport clones is not accepted

Primary closure rule:
- `AC.5` is the primary closure sprint only for remaining RPC/body usage
  convergence
- it must not reopen canonical shared type design that already closed in
  `AC.1`

## Governing Sources

- `docs/plan-phase-AC.md`
- `docs/phase-AC/sprint-AC1.md`
- current RPC/protocol code in `atm-core`, `atm-daemon`, and `atm-daemon-client`

## Prerequisites

- `AC.1`
- `AC.4`

## Out Of Scope

- no backend extraction work
- no transport-protocol redesign beyond envelope/body unification

## Deliverables

- `RpcEnvelope` is owned by `atm-daemon-client` as the transport-side shared
  envelope crate surface for this phase line; `atm-storage` is explicitly not
  an allowed owner because the envelope is transport, not storage
- the transport layer uses one generic envelope:

  ```rust
  pub struct RpcEnvelope {
      pub header: RpcHeader,
      pub body: bytes::Bytes,
  }
  ```

- message, roster, and task bodies decode into the canonical shared domain structs from `atm-storage`
- per-message transport clones are deleted unless a real semantic difference remains
- the same canonical `Message` struct is passed over RPC and into storage

## Ledger-Driven Convergence Targets

`AC.5` consumes the canonical types chosen in `AC.1` and collapses the
remaining duplicated body shapes around them.

Message-family convergence targets:

- `MessageEnvelope` transport/storage duplicates -> canonical `Message`
- `MailStoreMessageRecord` usage sites -> canonical `Message`
- mailbox metadata query wrappers -> `MessageQuery` or transport-level filters

Roster-family convergence targets:

- `RosterMemberRecord` usage sites -> canonical `RosterMember`
- do not let `ClaudeCodeRosterMember` reappear above backend-internal projection code

Task-family convergence targets:

- `TaskStoreTaskRecord` / `TaskStoreTaskMetadata` usage sites -> canonical `Task`
- task query wrapper bodies -> `TaskQuery`

Must remain outside this sprint’s storage contract work:

- transport traits from `boundary/mod.rs`
- config ingress / doctor surfaces
- outbound delivery-only request/response types unless they directly clone a canonical body

## Execution Checklist

Implementation order for `AC.5`:

1. Define the final generic RPC envelope shape and freeze header/body responsibilities.
2. Identify every RPC body that is a semantic duplicate of:
   - `Message`
   - `RosterMember` / `RosterSnapshot`
   - `Task`
3. Convert those operations to decode directly into the canonical shared structs.
4. Leave transport-only context in headers or operation wrappers, not in cloned body structs.
5. Delete transport clones after each family is migrated rather than keeping parallel bodies to the end.

Proof this sprint must leave behind:

- one canonical message body shape crosses RPC and storage
- roster and task bodies follow the same rule
- the remaining request/response types are transport operations, not cloned domain records
- any surviving canonical-type ambiguity after `AC.5` is a failure of `AC.1`,
  not a reason to create another shared model here

## Acceptance Criteria

- no new transport-only message clones remain where the shared canonical struct is sufficient
- RPC envelope headers carry transport concerns only
- RPC bodies decode into shared domain structs rather than backend- or transport-specific clones

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
- `python3 scripts/lint_boundaries.py`
- `rg -n "MailStore.*Request|MailStore.*Response|TaskStore.*Request|TaskStore.*Response|RosterStore.*Request|RosterStore.*Response" crates/atm-core crates/atm-daemon crates/atm-daemon-client -S`
- `rg -n "MessageEnvelope|MailStoreMessageRecord|RosterMemberRecord|ClaudeCodeRosterMember|TaskStoreTaskRecord|TaskStoreTaskMetadata" crates/atm-core crates/atm-daemon crates/atm-daemon-client -S`

## Required Document Updates

- `docs/phase-AC/sprint-AC5.md`
- `docs/phase-AC/readiness.md`
- `docs/project-plan.md`
- protocol and architecture docs that describe transport/body shapes
- create or update `atm-daemon-client` boundary TOML records to make its
  transport-envelope ownership explicit and to document whether it remains
  transport-only or consumes canonical `atm-storage` domain bodies without
  becoming a storage crate

## Risks And Watchouts

- if the generic envelope keeps transport-specific body clones, the type explosion will survive under a new name
- if transport metadata is pushed back into the shared domain structs, the layering will invert again
- if this sprint leaves canonical structs unused while transport clones still carry the real traffic, the unification is only cosmetic
