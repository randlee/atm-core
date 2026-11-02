# Intent — phase-bo (restart 2)

Rewritten 2026-11-01 by Rand with Claude. One page. Jev judges against `anchors.json`; this page is the why.

## The rule everything else follows from

**Python is for gates. Sequences are for agents.**

A lite hook checks facts and makes at most one beads-cache write. Anything that is "do A, then B, then C, and any step can fail" is not Python: it is a fenced bash block in the agent's task template, and the agent runs it, reads the error when a step fails, and reruns. This is the only way lite stays small. atm-bd-orchestration is 3–5k lines of Python for one more state than lite has because every hardening pass moved one more sequence into Python. phase-bo r1–r14 did the same thing in a week. Plan hardening must not do it again; ANC-064..067 make that mechanical.

## What lite is

Same bead state model, schema and `atm-bd` commands as orchestration; dev → QA, no sanity stage; one agent can run a phase. Orchestration is untouched (ANC-039). Shared code is new code in the shared location, used by lite the sprint it lands (ANC-040, ANC-045).

## The stack bead is an atomic ledger

Its assignee is the lock: one agent at a time rebases, pushes and records. Its layers are the record. The dev takes the lock, rebases onto the top, pushes, records its layer and releases, in one fenced block in the dev template, through `bdc` (beads-cache) and `git`. No script, no hook writes the ledger. Hooks only read it.

## The close contract

Dev, on success: finish the work; run the stack block; `atm task close completed` with the report.
Hook: check the report is present and the ledger holds the dev's layer; close the bead with that evidence; return `atm task close --template <close-template> --args <evidence>`.

Dev, on refusal: `atm task close refused` with a reason. Hook: reopen the bead (open, unassigned); return `atm task close --template <refuse-template> --args <reason>`.

QA, on pass: `atm task close completed` with the verdict. Hook: verdict sha is the target layer's head; close with that evidence.
QA, with findings: pour each finding through `bdc` in a fenced block (blocking findings block the sprint bead; non-blocking block the phase bead), then close with the report listing the poured ids. Hook: every blocking finding has its poured bead blocking the sprint; close. The QA bead cannot close until blocking findings are poured.

The QA bead is blocked by its dev bead. When the dev bead closes the QA bead is released and is assigned by the existing `atm task assign` path. The close hook does not assign anything.

## Out of scope

Changing orchestration. A jev-check package. A stack script. Retry or recovery doctrine (ATM does not die). Creating beads from a hook. Any hook writing the ledger.
