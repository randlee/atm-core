# BA.11 — ADR amendments and final task-surface documentation

| Field | Value |
| --- | --- |
| Wave | 6 (last) |
| Branch | `docs/ba11-adr-amendments` |
| Base | `integrate/phase-ba` |
| Stack | not stacked — independent PR, opened after every other sprint merges |
| Dependency | `must_follow` **every other BA sprint** by PR completion. It records what shipped; it cannot precede it. |
| Governed interface | records the ADR-061 MAJOR approval (R0); does not itself change an interface |
| recommended_agent | Cipher-311d |
| recommended_model | fast |

## Why this sprint exists

Three review findings converge on the same hole: phase BA changes governed
contracts that live in accepted ADRs, and **no sprint owned the amendments**.

- **PLAN-CRIT-003** — BA.1, BA.3, BA.8 and BA.9 all contradict ADR-062's
  accepted state machine, and BA.7 (the only documentation sprint) explicitly
  forbids `docs/adr/*` edits. An accepted ADR left describing behaviour the
  phase deleted is worse than no ADR: the next phase reads it as current.
- **PLAN-CRIT-004** — BA.6 changes ADR-054's queue contract (added message-row
  state, message-before-task precedence) with no amendment.
- **PLAN-CRIT-021** — BA.7 documents the `atm task` surface *before* R1 and R2
  were decided and is explicitly written to describe it "as planned". After
  BA.5 and BA.9 ship six verbs, nothing updates it to describe what exists.

This sprint is deliberately **last**. An amendment written before the code
lands is a prediction, and phase AZ produced several that then disagreed with
what shipped. Every deliverable here is written against merged behaviour and
verified by reading the merged code, not the sprint docs.

Documentation and ADRs only. No code, no tests, no boundary manifests, no
schema.

## Deliverables

### D1. ADR-062 amendment — the task state machine as phase BA leaves it

ADR-062 is amended, not superseded: its ledger ownership, its audit-replay
claim and its `TaskStore` framing survive. What phase BA falsifies:

| ADR-062 text | Falsified by | Amendment |
| --- | --- | --- |
| "events are `Assigned`, `Acked`, and `Completed`" | BA.1 (ack leaves the task domain), BA.9 (`Started`, `Reassigned`) | new event set, with `Acked` no longer a task event |
| transition table row `Assigned` + `Acked` → `Active` | BA.1, BA.9 R1(a) | `Assigned` + `Started` → `Active`; acknowledgement no longer transitions |
| "only the acknowledgement writer operation applies acknowledgement" | BA.1 | deleted |
| "Acknowledging an assigned task rejects when another task is active" | BA.1, BA.3 D2 | the one-active invariant moves to `one_active_task_per_agent`, enforced on start |
| states `Assigned`, `Active`, `Complete` | BA.3 D4 | `Complete` carries a typed `TaskCloseOutcome`; the state set gains no member but the terminal state gains a discriminant |
| replay claim per `(team, task_id, assignee)` | BA.3 D1 | per `(team, task_id)`; the assignee is an attribute of the row, not part of identity |
| "selects the oldest active task (or the oldest assigned task when none is active)" | BA.3 D3 | selection is by `position`, active-at-position-1 |
| reminder cycle as drain-then-check edge logic | BA.4 | continuous evaluation of the invariant, with its five arms |
| "the first escalation is eligible after 60 seconds … every 10 minutes" | BA.8 D1 | terminal threshold with a durability gate; at-least-once **per daemon epoch** (R-BA.8 decision (a)) |
| "A missing or ambiguous lead suppresses that audit event without suppressing configured escalation-recipient fan-out" | BA.8 D4/D18 | unchanged in intent, but the audit must now record recipient writes so suppression can be earned without a lead |

The amendment restates the **full** transition table and the **full** event
set inline. A reader must never have to diff two documents to learn the
current contract.

### D2. ADR-054 amendment — the queue contract after BA.6

Two distinct edits, in two distinct sections, so they do not collide:

- BA.2 already owns ADR-054's **nudge title metadata** section and lands its
  edit with the code. BA.11 does not touch it.
- BA.11 owns the **queue mechanism** section: each message-row column BA.6
  added (name, type, default, writing transitions — copied from the shipped
  schema, not from BA.6's sprint doc), and the four normative scheduling
  rules from BA.6 D4/D2a. ADR-054's bare-CLI pull contract is unchanged and
  the amendment says so explicitly.

### D3. ADR-064 — the one-way task schema migration (the R0 approval record)

A new ADR, in the shape of ADR-063 D6, recording:

1. The ADR-061 **MAJOR** classification of BA.3's schema change and the
   reasoning (ADR-061:64 — renaming/removing a field; the previous binary
   cannot operate against the migrated database).
2. The decided shape: **one-way, version-gated, no bridge, no dual
   authority.** The old binary refuses to open a migrated database; rollback
   is restore-from-backup. This is deliberately *not* ADR-063's coexistence
   model, and the ADR states why the two phases chose differently.
3. **Rand's explicit recorded approval**, quoted with its date. Without that
   quote this ADR is not completable — see Non-closure.
4. The `schema-reviewer` sign-off reference for BA.3 and BA.10.
5. The consequence stated plainly: **this is a breaking upgrade with no
   supported downgrade path.**

### D4. Final task-surface documentation

Replace BA.7's "as planned" wording with the shipped surface: the six verbs
(`assign`, `start`, `close`, `reassign`, `list`, `show` — names taken from
what BA.5 and BA.9 actually shipped), the three-stage close, the authority
matrix, the mandatory report, and the deferred-delivery default. Touches
`CLAUDE.md`, `docs/agent-conventions.md`, `docs/team-protocol.md`.

Also carries the **verb-count amendment's downstream text** if BA.9's own
amendment left any document still asserting five verbs.

## Acceptance criteria

1. ADR-062 contains one amendment section restating the complete event set and
   the complete transition table, and no remaining sentence in the document
   describes acknowledgement as a task transition. Gate: reviewer greps
   `Acked` across the file and confirms every hit is historical narrative.
2. Every row of D1's falsification table is addressed in the amendment.
3. ADR-054's queue-mechanism amendment names each added message-row column
   with type, default and writing transitions, and each name matches the
   shipped schema. Gate: reviewer diffs the column list against the merged
   migration.
4. ADR-054's nudge-title section is byte-identical to what BA.2 merged.
5. ADR-064 exists, is `Accepted`, and quotes Rand's approval with its date.
6. ADR-064 states the no-downgrade consequence in its Consequences section.
7. No agent-facing document describes the `atm task` surface in the future
   tense, and the verb list matches the shipped CLI. Gate: reviewer runs
   `atm task --help` against the merged binary and compares.
8. `docs/adr/INDEX.md` lists ADR-064 and marks ADR-062 and ADR-054 amended.
9. No code, test, schema or boundary-manifest file is in the diff.

## Required validation

- the documentation lint path, not full CI — doc-only change
- `just lint` where it covers markdown
- a reviewer pass that reads the **merged** code for each claim, not the
  sprint docs

## Non-closure

**AC5 is a hard stop, not a checklist item.** If Rand has not recorded
approval of the ADR-061 MAJOR classification by the time this sprint opens,
ADR-064 cannot be written as `Accepted` and the sprint does not close. The
correct action is to escalate, not to draft an approval record on his behalf.
BA.3 and BA.10 are gated on the same approval and should already have
surfaced it much earlier; this criterion is the backstop, not the gate.

This sprint writes no ADR for anything phase BA did not ship. If a deliverable
was dropped during the phase, its amendment row is deleted, not written
speculatively.
