# BA.11 — Final verification and code-verified documentation

| Field | Value |
| --- | --- |
| Wave | 6 (last) |
| Branch | `docs/ba11-final-verification` |
| Base | `integrate/phase-ba` |
| Stack | not stacked — independent PR, opened after every other sprint merges |
| Dependency | `must_follow` **every other BA sprint** by PR completion, and **`must_follow` BA.0** in the sense that matters: BA.0 is the decision this sprint verifies against |
| Governed interface | verifies the classifications BA.0 recorded; changes none |
| recommended_agent | Cipher-311d |
| recommended_model | fast |

## Why this sprint exists

BA.0 decided the contracts. This sprint proves the code matches them, and
writes the user-facing documentation that could not exist until the surface
did.

**This sprint holds no normative content (R2-CRIT-004).** An earlier revision
gave it the ADR-054 and ADR-062 amendments and the ADR-064 approval record,
which inverted ADR-054's "later sprints implement, never define" rule and
would have recorded a MAJOR approval after the irreversible migration. Those
moved to **BA.0**. What remains here is verification and documentation —
work that genuinely cannot be done in advance, and that a decision written in
advance can still fail to survive.

Documentation only. No code, no tests, no schema, no ADR *decisions* — a
correction to an ADR whose text the implementation proved wrong is escalated
to BA.0's owner as a follow-up PR, not written here.

## Deliverables

### D1. Verify the shipped system against BA.0's ADRs

One pass per amended document, reading the **merged code**, not the sprint
docs:

| check | source of truth | failure action |
| --- | --- | --- |
| the shipped event set and transition table match ADR-062's amendment | `atm-storage/src/task_state.rs` | file a follow-up PR against BA.0's ADR **and** report the divergence; do not silently re-word the ADR to match the code |
| the shipped message-row columns match ADR-054's amendment exactly — names, types, defaults, write sites | the merged migration | same |
| the shipped migration shape matches ADR-064 | the merged migration path | same; a divergence here is a release blocker, not a documentation defect |
| every entry in BA.0's exhaustive additions list shipped, and nothing outside it did | `git diff integrate/phase-ba...develop` on the phase | budget breach — stop and escalate |

The asymmetry is deliberate: when code and ADR disagree, the **ADR is the
decision** and the code is the defect. This sprint reports; it does not
reconcile by editing the ADR.

### D2. Close the ADR index and cross-references

`docs/adr/INDEX.md`, and every ADR whose "Relates to" line should now name
ADR-064. Mechanical, but nobody owns it otherwise.

### D3. Final task-surface documentation

Replace BA.7's "as planned" wording with the shipped surface: the six verbs
`assign`, `start`, `close`, `reassign`, `move`, `list`, `events` — BA.5's five
plus BA.9's two — the three-stage close, the R3 authority matrix, the
mandatory report, and the deferred-delivery default.

*Two earlier revisions of this list were wrong in opposite directions: the
first invented a `show` verb that appears nowhere in BA.5 and dropped `move`
and `events`; the second folded reassignment into `assign`. Verify the list
against `atm task --help` on the merged binary and against
`cli_surface_baseline.json`, never against this paragraph.* Touches
`CLAUDE.md`, `docs/agent-conventions.md`, `docs/team-protocol.md`.

Also carries the **verb-count amendment's downstream text** if BA.9's own
amendment left any document still asserting five verbs.

## Acceptance criteria

1. The shipped event set and transition table match ADR-062's amendment
   exactly. Gate: reviewer reads `task_state.rs` at the merged head and
   compares element by element.
2. The shipped `mail_message_states` columns match ADR-054's amendment
   exactly — names, types, defaults, write sites. Gate: reviewer diffs the
   merged DDL against the ADR.
3. The shipped migration matches ADR-064's decided shape. A divergence is
   reported as a **release blocker**, not a documentation defect.
4. Every entry in BA.0's exhaustive additions list shipped, and the phase
   diff contains no addition outside it.
5. No agent-facing document describes the `atm task` surface in the future
   tense, and the verb list matches the shipped CLI exactly — seven verbs, no
   `show`. Gate: reviewer runs `atm task --help` against the merged binary
   and diffs against `cli_surface_baseline.json`.
6. `docs/adr/INDEX.md` and every affected "Relates to" line reference
   ADR-064.
7. No code, test, schema, ADR-decision or boundary-manifest file is in the
   diff.
8. Every divergence found under criteria 1-4 is reported with a follow-up PR
   against BA.0's ADR, or closed as "no divergence" in writing. Silence is
   not a pass.

## Required validation

- the documentation lint path, not full CI — doc-only change
- `just lint` where it covers markdown
- a reviewer pass that reads the **merged** code for each claim, not the
  sprint docs

## Non-closure

**This sprint does not write ADR decisions.** When the code and an ADR
disagree, the ADR is the decision and the code is the defect. Re-wording the
ADR to match what shipped would make every one of these criteria
unfalsifiable — which is exactly what putting the amendments last would have
done (R2-CRIT-004).

**It also does not rescue an unruled R0.** If ADR-064 does not exist because
Rand has not ruled, this sprint reports that the phase shipped a migration
with no approval record — a release blocker — rather than drafting an
approval on his behalf. BA.0 and BA.3/BA.10 are gated on the same ruling and
should have stopped long before this point.
