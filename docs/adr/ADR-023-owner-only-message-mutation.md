# ADR-023 — Owner-Only Message Mutation

| Field | Value |
|---|---|
| ID | ADR-023 |
| Status | **Accepted** |
| Date | 2026-07-10 |
| Deciders | Rand Lee |
| Relates to | ADR-001, ADR-012, ADR-018 |
| Supersedes | — |

---

## Context

The retained ATM command surface had drifted into an unsafe shape where
mutation and inspection were mixed together:

- `atm read --no-mark` kept a non-mutating inspection path hidden behind the
  mutating read command
- some mailbox-facing commands still exposed caller impersonation flags even
  though they mutated another member's mailbox state
- documentation did not consistently distinguish inspection-only commands from
  owner-only mutation commands

That drift made message state unsafe to reason about. A caller could inspect or
mutate mailbox state through multiple overlapping paths, and mutating commands
could no longer fail closed on unresolved caller ownership.

## Decision

The accepted mailbox contract is:

- `atm peek` is the explicit non-mutating mailbox inspection command
- `atm list` remains an inspection-only metadata query surface
- only inspection-only surfaces may inspect another member with `--as`
- `atm send`, `atm read`, `atm ack`, and `atm clear` are owner-only mutating
  commands
- owner-only mutating commands must resolve caller identity and team from the
  documented caller-context matrix before daemon dispatch or in-process
  execution
- mutating commands must not expose a caller impersonation flag

The accepted command split is therefore:

- inspection-only:
  - `atm peek`
  - `atm list`
- owner-only mutating:
  - `atm send`
  - `atm read`
  - `atm ack`
  - `atm clear`

## Enforcement

This ADR is enforced only while all of the following remain true:

- the authoritative caller-context matrix in `docs/requirements.md` §4.1 keeps
  `peek` and `list` as the only mailbox/message inspection surfaces that may
  accept `--as`
- CLI help and command docs do not advertise mutating impersonation
- downstream caller-owned request DTOs in `atm-core` carry resolved required
  caller context as required fields for mutating commands
- mailbox-inspection code paths do not mutate `read`, `seen`, or
  acknowledgement state

## Boundary Conditions

This ADR does not change:

- target-recipient addressing for `atm send`
- sender filtering via `--from` on inspection surfaces
- the durable sender-owned acknowledgement model introduced separately by
  `ADR-022`

This ADR also does not authorize any fallback caller-context source beyond the
documented CLI/env matrix.

## Consequences

### Positive

- mailbox inspection and mailbox mutation are separated into explicit command
  surfaces
- mutating commands fail closed when caller ownership is unresolved
- callers can inspect another member's mailbox without silently mutating it

### Negative

- workflows that relied on mutating impersonation must move to explicit
  owner-run commands or inspection-only `peek`
- documentation and help text must stay aligned with the stricter split

## Review Conditions

This ADR must be revisited if ATM introduces any new mailbox/message command
surface that:

- mutates mailbox state without a resolved owner identity/team
- reintroduces a non-mutating read variant under `atm read`
- permits impersonation on a mutating command
