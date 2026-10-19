# Intent - phase-bj: mechanical close flow

Written 2026-10-18 by fenix from Rand's rulings of 2026-10-18 (atm-core #1873 comment 1, atm-bd-orchestration #340, docs PR atm-bd-orchestration #342). One page at most. Jev questions are judged against the anchors, not this document.

## What this plan is for

Dev work is not complete until sanity verifies it, and nothing between a dev
push and the QA assignment should need the lead. Today the dev closes its own
task on push, the lead hand-assigns every sanity and QA task, and atm-bd has
never issued an assignment because its dispatch reads branch and worktree
from beads the pour never wrote them to. This phase makes the dev, fix,
sanity and qa closes one script, `atm-task-close.py`, that verifies the
minimum (open PR, pushed head, descends from the stack top), closes the bead,
reports to the lead and assigns the next bead as the phase lead; the dev's
task stays open until sanity passes.

## What success looks like

- A dev or fix bead closes only through the hook; the dev's ATM task is closed by the sanity hook on PASS and the dev waits meanwhile.
- Sanity and QA assignment never need the lead: the hook assigns, atm-bd assigns a ready bead with no task, both as the phase lead.
- A sanity FAIL reopens the bead and sends the dev `sanity-fail` on the same open task; at `max_sanity_rounds` (default 5) the sanity bead waits on the lead.
- The pour writes branch, worktree, pr_target and difficulty to every bead of a sprint family and a pydantic model per bead kind validates the whole phase.
- atm-core issue #1873 (task transition hooks, 1.6.2) is the long-term home; the skill script stands in until it ships.
- Sanity links the layer on PASS under the stack bead's claim: the stack bead holds the whole stack as ledger and lock, first PASS wins the top and the loser gets `rebase-required` (no round counted); a sanity-stack subagent handles one next-top layer per spawn, sequentially, with a lean mode and a lock timeout that never blocks the team.

## What is deliberately out of scope

- Mechanical dev or fix assignment: the lead picks the dev (ANC-060).
- Whole-stack verification in the hook: the lead owns stack hygiene (ANC-054, ANC-062).
- The small phase-bi console fixes (self-addressed send, icons, agent rows): they close in phase-bi.

## Anchors

The phase-bi anchors (ANC-001..050, REQ-BI6-AC6) are carried unchanged in
`anchors.json` so Jev judges phase-bj items against the same console
rulings; ANC-051..086 are this phase's; ANC-063 and ANC-074 are retired and replaced by ANC-078..083 and ANC-084..086, respectively (ANC-062 retired to ANC-065). Rendered to `anchors.md`.
