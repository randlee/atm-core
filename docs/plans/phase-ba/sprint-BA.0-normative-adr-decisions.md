# BA.0 — Normative ADR decisions, before any implementation

| Field | Value |
| --- | --- |
| Wave | 0 — before every code sprint |
| Branch | `docs/ba0-normative-adr-decisions` |
| Base | `integrate/phase-ba` |
| Stack | not stacked — independent PR, merged before wave 1 opens |
| Dependency | none. **It blocks everything else.** |
| Governed interface | it is the *decision record* for all three; it changes no code |
| recommended_agent | Cipher-311d |
| recommended_model | fast |

## Why this sprint exists

An earlier revision put every ADR amendment in BA.11, at the end, reasoning
that an amendment written before the code is a prediction. That reasoning is
wrong here, and R2-CRIT-004 says why:

- **ADR-054 states that later sprints *implement* its marker/`read = 0`
  contract and never redefine it.** BA.6 changes what stays claimable after
  handoff. If the amendment is written afterwards by copying BA.6's shipped
  schema, the implementer defined the contract and the ADR ratified it. That
  is the inversion ADR-054 exists to prevent.
- **ADR-061 D3 requires a MAJOR change's approval to exist *before plan
  approval*.** An ADR-064 marked `Accepted` after BA.10 has run records
  approval for something irreversible that already happened.
- **ADR-062's state machine is contradicted by BA.1, BA.3, BA.8 and BA.9.**
  Four sprints implementing against a document that still describes the old
  machine will each interpret the gap differently.

So the phase splits the work in two. **BA.0 decides. BA.11 verifies.** Every
code sprint implements against an accepted decision; nothing generates one.

Documentation and ADRs only. No code, no tests, no schema, no manifests.

## Deliverables

### D1. ADR-062 amendment — the task state machine phase BA will build

Amended, not superseded: ADR-062's ledger ownership, `TaskStore` framing and
audit-replay claim survive. What phase BA changes:

| ADR-062 text | changed by | amendment |
| --- | --- | --- |
| events `Assigned`, `Acked`, `Completed` | BA.1, BA.9 | `Assigned`, `Started`, `Reassigned`, `Completed`; `Acked` is no longer a task event |
| `Assigned` + `Acked` → `Active` | BA.1, R1 | `Assigned` + `Started` → `Active`; acknowledgement never transitions |
| "only the acknowledgement writer operation applies acknowledgement" | BA.1 | deleted |
| "Acknowledging an assigned task rejects when another task is active" | BA.1, BA.3 D2 | the one-active invariant moves to the `one_active_task_per_agent` index, enforced on start |
| terminal state `Complete` | BA.3 D4 | `Complete` carries a typed `TaskCloseOutcome` (`completed`, `refused`, `cancelled`) |
| replay per `(team, task_id, assignee)` | BA.3 D1 | per `(team, task_id)`; assignee is an attribute of the row |
| "selects the oldest active task (or the oldest assigned task when none is active)" | BA.3 D3, BA.4 D1 | selection is by `position`, one due task per member, active-at-position-1 |
| reminder cycle as drain-then-check edge logic | BA.4 | continuous evaluation of the invariant, six dispositions, with the staleness rule for `Unknown` |
| escalation eligibility "after 60 seconds … every 10 minutes" | BA.8 D1 | terminal threshold with a durability gate; **at-least-once per daemon epoch** |
| the implicit "an Active agent is never diverted" | BA.4 D5a, R2-CRIT-011 | **no nudge is *admitted* once a member is Active; at most one already-admitted emit may still land.** Stated as a weakening, in these words |

The amendment restates the **complete** event set and the **complete**
transition table inline. A reader must never diff two documents to learn the
current contract.

### D2. ADR-054 amendment — the queue contract BA.6 will implement

- BA.2 owns ADR-054's **nudge title metadata** section and lands that edit
  with its code. BA.0 does not touch it.
- BA.0 owns the **queue mechanism** section: the two columns BA.6 will add
  (`delivery_mode`, `handed_off_at` — name, type, default, writing
  transitions, taken from BA.6 D1), the four normative scheduling rules from
  BA.6 D4/D2a, and the statement that the selection predicate no longer reads
  `nudge_pending_at`.
- The additive-MINOR classification of that change under ADR-061, with its
  `STORAGE_SCHEMA_VERSION` obligation named.
- ADR-054's bare-CLI pull contract is unchanged, and the amendment says so.

BA.6 then implements this text. BA.11 diffs the shipped DDL against it.

### D3. ADR-064 is **not** this sprint's — it is in the plan PR

ADR-061 D3 requires the approval *"before plan approval"*, so the migration
decision cannot be a phase deliverable at all. `docs/adr/ADR-064-…` ships in
the plan PR (#1397) as `Proposed`, and Rand's decision on its D4 sets its
status before any sprint opens.

This sprint's only obligation to it: if D4 resolves to option (i), the
additive shape, then BA.0's ADR-062 amendment must describe writer-enforced
uniqueness rather than a schema-enforced primary key, and BA.3/BA.10 are
rescoped before they open.

### D4. The exhaustive additions list

The complexity budget promises zero new traits, capabilities, tables, state
machines and id types. Several deliverables add *types and fields* that are
none of those, and they were accumulating unrecorded. BA.0 writes the single
authoritative list: `TaskCloseOutcome`, `TaskOp`, `WriteRequest.task_op`,
`AsyncTaskLedgerReader::recent_closed_tasks`, the runtime `escalation_epoch`
field, and `mail_message_states.delivery_mode` / `handed_off_at`.

Anything not on that list at phase end is a budget breach, and a sprint that
needs a new entry stops and amends this list first.

## Acceptance criteria

1. ADR-062 contains one amendment restating the complete event set and the
   complete transition table; no sentence in the document describes
   acknowledgement as a task transition.
2. Every row of D1's table is addressed.
3. The weakened no-diversion guarantee appears in ADR-062 in the words BA.4
   D5a uses. It is not softened, and it is not omitted.
4. ADR-054's queue-mechanism amendment names both columns with type, default
   and writing transitions, and classifies the change MINOR with its version
   obligation.
5. ADR-054's nudge-title section is untouched by this sprint.
6. ADR-064's D4 has been decided and its status is no longer `Proposed`, and
   this sprint's amendments describe the shape that was chosen. If D4 is
   still open, this sprint does not open.
7. The exhaustive additions list exists and every entry names the sprint that
   ships it.
8. `docs/adr/INDEX.md` reflects the amendments.
9. No code, test, schema or boundary-manifest file is in the diff.

## Required validation

- the documentation lint path, not full CI
- `schema-reviewer` reviews this sprint's classifications before it merges —
  this is the sprint where a wrong classification is cheap to fix

## Non-closure

This sprint decides; it does not verify. Whether the code matches these
decisions is **BA.11**, and BA.11 is required precisely because a decision
written in advance can be missed by an implementation. Neither sprint is
redundant with the other, and neither may absorb the other.

If a code sprint discovers that a decision here is unbuildable, it stops and
amends this sprint's ADR through a follow-up PR. It does not build something
else and leave the ADR wrong.
