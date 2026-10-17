---
name: plan-size-and-shape
version: 0.6.0
description: What to do when a plan bead or a phase is too large - size is a symptom, not a verdict; the triage order is unapproved work, then prose and ceremony, then split last; deterministic shape metrics measure it before anyone decides.
---

# Plan size and shape

Oversize is a symptom, not an instruction to split. The causes are work nobody asked for, prose and process ceremony, and too much work in one bead, often all three. Splitting a bloated bead multiplies the bloat: twelve bloated beads become thirty-six under the cap. Judge the cause, in order, and report what you found to the user.

## The rule

`PLAN.TOO_LARGE` (a bead whose request would not fit one Jev call even with every long code block reduced to signatures; see Long code) stops the gate for that bead and reports its size by section and prose versus code. Judge in this order only:

| Step | Remove | How you tell | Who decides |
| --- | --- | --- | --- |
| 1 | **Unapproved work**: anything no anchor, requirement, ADR, or user-approved issue asks for | Planning-gate verdicts on the scope anchors (for example "every deliverable addresses a named issue from the approved list"), the `shape` metrics' issue references, and the deliverables list | Remove it. Never keep it because it seems useful; keep it only if the change cannot succeed without it, justified in one line and listed in the planning-complete summary |
| 2 | **Prose and ceremony**: restated rules, cross-reference tables, approval qualifiers, process artifacts, rationale that repeats the intent, text duplicated across beads | `shape` (its repeated-paragraphs table), prose-to-deliverable ratio, and the reviewing-ceremony skill when installed, which locates ceremony item by item with Jev | Cut it. A bead describes deliverables and the design to build them; the rules live in the intent, the anchors, and the repo's guidelines |
| 3 | **Split**: approved work still too large after 1 and 2 | Only now | Split along deliverables, never along the size cap, and re-run 1 and 2 on each piece |

Never split, trim or raise a limit to pass the gate. A bead that still does not fit after steps 1 and 2 is more than one sprint.

The same order applies to the phase. Fourteen sprints for a point release is a symptom: ask which sprints no approved issue requires, then which beads carry the same ceremony, and only then whether any approved sprint is too big.

## What Jev reads

Jev accepts far more than a reasonable bead contains but does not read it evenly: a planted contradiction scored well near the top of a bead and poorly deep in its design (measured-behaviour.md). Consequences:

- The planning gate judges a bead's title, description and acceptance criteria. The design holds the bulk and changed no verdict when included. A bead whose description and acceptance criteria alone exceed the cap is too large by any reading.
- What a bead has to say about an anchor goes first, in the description. A requirement honoured only in paragraph forty of the design is, to Jev, not honoured.

The planning-gate cap (24,000 bytes, `jev-api.md`) is "what Jev reads reliably", not a transport limit. `shape` reports the whole bead, including the design: a 40k-character design on a sprint bead is a symptom even when the judged sections fit.

## Measure before judging: `shape`

Counting and sizing are the script's job, never Jev's. `shape` reports, per bead and for the set:

```bash
python3 "$S" shape --beads <dir of bd show --json files | --children-json <file>> [--anchors "$A"] [--format md]
```

| Metric | Why it matters |
| --- | --- |
| characters per section (description, design, acceptance, notes), total, after code reduction (every long block reduced: the smallest the gate can send), and the design ratio | Where the bulk is; designs of 40k to 70k characters are design documents, not sprint beads. Description plus acceptance is what the gate judges |
| code lines versus prose lines | Long code can be reduced to signatures when a bead does not fit (Long code); prose is never cut and costs judgment |
| bullets and table rows | Ceremony arrives as tables and checklists |
| GitHub issue, REQ, ADR and anchor references | A bead naming no approved issue is the first candidate for step 1 |
| paragraphs duplicated across two or more beads (normalised) | Ceremony restated in every bead; cut it once from all |
| sprint count against the intent's issue count | Phase shape at a glance |

`shape` never fails a gate; it tells the triage where to look. Run it first when a plan feels out of control.

## What the gate itself judges

The gate helps with step 1: a scope anchor such as "Every phase deliverable is a targeted addition that addresses a named issue from the approved list; nothing is added that those issues do not require" makes unapproved work a `contradicted` verdict on the bead that carries it. Lock one for every phase. Step 2 is not an anchor question; use the reviewing-ceremony skill, when installed, on the plan beads (`bd show <ids> --json | ...`); it returns the items matching ceremony patterns and their locations.

## Long code

Long code in a design is never by itself a reason to refuse a bead. A bead that fits one Jev request (`--max-item-chars`, and `--max-request-bytes` for every anchor judged) is sent whole, code included: Jev judges code it can read better than code it cannot (measured-behaviour.md). Only a bead that would not otherwise fit has code reduced, and as little as possible: fenced blocks longer than `--max-code-lines` (default 8) are sent as their signature lines plus a marker saying how many lines were not sent, one more block at a time, longest first, until the bead fits. Jev sees that the reduced code exists without reading it, and the plan keeps the full code. A bead that does not fit with every such block reduced is `PLAN.TOO_LARGE`. This is the only reduction the planning gate applies; nothing said in prose is removed. The run records `item.chars_before_reduction` and `item.code_reduced` (the blocks and lines actually reduced; 0/0 when the bead was sent whole).

## Where the cap comes from

The cap (`--max-request-bytes`, `jev-api.md`) is the size beyond which judgments on planted contradictions stopped being reliable, with margin; not Jev's hard limit. A bead under the cap is not thereby reasonable; one over it is not thereby wrong. Read `shape` either way.

## Posting the triage

When a bead is refused, post on it: its `shape` row, the scope-anchor verdicts for the parts that fit, the ceremony findings if the reviewing-ceremony skill was run, and the proposed removals in the order above, as questions to the user where approval is uncertain. Then edit, re-run `shape`, re-run the gate. A split comes last, for the remaining approved work only.
