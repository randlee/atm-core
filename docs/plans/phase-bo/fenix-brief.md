# Brief for fenix — phase-bo restart 2 (2026-11-01)

Read `intent.md` and `hooks/*.md` first. They are decided. Your job is sprints and the Jev gate, not design.

## Produce
1. Anchor edits to `anchors.json` (section A below), via plan-anchor. Commit the Jev run for each new anchor's fixtures.
2. Sprints, each small enough that its diff is reviewable in one sitting:
   - bo-9: `atm-bd-types` 0.2.0 — only the names `hooks/*.md` use. Delete from the stub every name no hook or doc uses (ANC-068). Add `FINDINGS_NOT_POURED`, `STACK_NOT_RECORDED`.
   - bo-10: the three hooks, each conforming to its `hooks/*.md` and nothing more; tests drive `hook()` with in-memory fakes; one test per Transitions row; one test per Errors row asserting the fix text.
   - bo-11: templates — the dev template's fenced **stack block** (bdc lock → rebase onto top → push → bdc record+release) and the QA template's fenced **pour block** (bdc pour per finding with the right blocker). These blocks are the sequences; they are not Python.
   - bo-12: `packages/jev-check` per `design/jev-check-proposal.md`, complete, then its lite caller (section B). Lands BEFORE bo-10.
   - bo-13 (atm-core, unchanged): typed refusal on the wire + audit log.
   - bo-14: one live lite phase.
   Upstream gate: beads-cache pour/create op (randlee/beads-cache#104) blocks the QA pour block only. Nothing in the hooks waits on it.

## Do not (each of these was invented in r1–r14 and is retired)
- `jev-check` is built exactly to `design/jev-check-proposal.md`: Rand's spec for the one Jev client that replaces every existing caller (orchestration's `jev_client.py`, plan-anchor's, the gate's `ask_jev`). Every deliverable in the proposal is in scope: check TOML config, the three extractors, Haiku and Luna backends, typed result, sessions, the lite caller. Do not trim it and do not copy the old client's code. Existing callers migrate to it in later sprints. Sessions and the Haiku/Luna backends are not optional: Jev returns a choice and a confidence with no rationale. The same request is run on Haiku or Luna, which return an answer and a session id; where their answer differs from Jev's you re-prompt that session to find out why, and tune the question set from that. It fixed the biggest tuning problem on 2026-11-01.
- No stack script (`atm-bd-lite-stack.py`). The stack is a fenced block in the dev template.
- No `qa_handoff`, no cron dispatching QA, no hook returning assign actions. The QA bead is released when its dev blocker closes and is assigned by the existing `atm task assign` path.
- No hook writes the stack bead (ledger). No hook claims or releases anything.
- No hook creates a bead. QA findings are poured by the QA agent through bdc.
- No retry/recovery doctrine, no idempotency keys in hook logic. ATM does not die.
- No `BdCli`, no `bd`/`gh` subprocess in hooks; `git` only as a read, and only if a hook spec's Facts table names it (none does today).
- No embedding hook code in sprint docs. Sprint docs link to `hooks/*.md` and to the file path.
- Do not add a guard, write or arm to a hook that its `hooks/*.md` does not list. If the spec is wrong, say so and stop; do not fix it in code.

## A. Anchor edits

Retire (reason: describes the bo-1..7 design; conflicts with ANC-063 and intent.md restart 2):
ANC-002 (check "one hook file"), ANC-007 (close-vars), ANC-008, ANC-009, ANC-010, ANC-021 (git/gh stack gate in the hook), ANC-004 fixture (close-vars helpers), ANC-017 check and ANC-020 fixture (Profile enum; conflicts ANC-061), ANC-056 context ("bd show"), ANC-060 fixture ("gh pr view" in the hook), ANC-031/012/027 fixtures naming `lite_cron.py` (conflicts ANC-032). Resolve ANC-013 against ANC-030/039 or retire it again.

Add:
- **ANC-064 Gate, not sequence.** A lite hook makes at most one beads-cache write. Check: count of `beads.update(`/`beads.create(`/`beads.claim(`/`beads.release(` call sites per hook file ≤ 1, and `create`/`claim`/`release` count = 0. Fail fixture: a close hook that calls `beads.update` twice (record layer, then close).
- **ANC-065 Hooks never write the ledger.** Check: no hook file passes a stack bead to a write; grep for `stack_bead` in a hook file appears only as an argument to `beads.show(` / `cache.get(`. Fail fixture: `BeadPatch.record(stack_bead, …)` in any hook.
- **ANC-066 No subprocess in a hook.** Check: grep for `subprocess`, `BdCli`, `GitCli`, `AtmCli`, `gh `, `bd ` in `skills/atm-bd-lite/scripts/atm-bd-task-*.py` is empty. Fail fixture: any of them.
- **ANC-067 Sequences live in templates.** Every multi-step procedure an agent must run (stack, pour) is a fenced bash block in a template under `skills/atm-bd-lite/templates/`. Check: the dev template contains one block whose lines are, in order, bdc lock, git rebase, git push, bdc record; the QA template contains one block with bdc pour. Fail fixture: a Python file that performs those steps.
- **ANC-068 Stub carries only used names.** Check: every `ErrorId` member, class and method in the generated `.pyi` is referenced by a hook file or a `hooks/*.md` table. Fail fixture: `STACK_CLAIM_LOST` with no caller.
- **ANC-069 Stack bead is the ledger.** Its assignee is the lock; its layers are the record; only the fenced stack block writes it, through bdc, under the lock. Check: ANC-065 plus the ANC-067 block shape.

Keep: ANC-003, 019, 048–054 (one file per hook, Transition enum, exhaustive match, one row per arm, Jev both directions), 055, 057, 058, 061, 062, 063.

## B. The lite gate (bo-12) — jev-check's lite caller (proposal deliverable 7)

The gate is `atm-bd-lite-conformance.py` from the proposal: it loads `.atm-bd/atm-bd-lite/jev-checks/task-{assign,start,close}.toml`, runs jev-check, and maps the result to gate findings. The deterministic checks below run in the caller before Jev. Arm/refusal extraction is the proposal's `python-branches` extractor and the spec tables its `table-column` extractor — in jev-check, not in the caller.

jev-check acceptance that the gate depends on: a request with code larger than the cap is refused with `INPUT_TOO_LARGE` and no request is sent; a fixture check produces exactly ONE request (asserted on a fake transport); the request JSON for the task-close gate is asserted byte-exact.

`skills/atm-bd-lite/scripts/atm-bd-lite-gate.py <hook.py> <hooks/spec.md>`:
1. `ast`: list the hook's `match` arms (case patterns + line) and every `return HookReply.refused(Refusal(ErrorId.X…))` / `.pass_through()` / `.not_applicable()` / `.handled(` site. List `beads.*(` call sites.
   *Proposed by fenix for Rand:* Forwarded refusals (refused(<Refusal value>)), from_result, and __main__ are not sites; they are covered by the Errors table's lead row.
2. Parse the spec's Transitions and Errors tables (markdown rows).
3. Deterministic checks (fail before Jev): rows == arms (ANC-052); writes ≤ 1 (ANC-064); ANC-065/066 greps.
4. One Jev request, two question sets, each a choice question: for every row, "is this row implemented by exactly one arm, with no additional check or write?" (ANC-053); for every arm and refusal site, "is it listed as a row or an Errors entry?" (ANC-054). State = the hook source + the spec. Fail fixtures for the question set: a close hook arm that also records a layer (expected: row implemented = no, because of the additional write); an unlisted `STACK_HELD` refusal (expected: listed = no).
5. Write `anchor-runs/hooks/<hook>.json`; exit nonzero on any `no`.
6. The gate runs on every push that touches `skills/atm-bd-lite/scripts/` or `skills/atm-bd-lite/templates/`, including every dogfooding fix after bo-14. A fix that adds a line to a hook must still satisfy rows == arms, writes ≤ 1, ≤ 100 lines, and both Jev question sets. This is what keeps dogfooding from adding Python: a fix that cannot pass the gate is a fix to a template or to the spec (via Rand), not to a hook.

Acceptance: the gate passes on the bo-10 hooks; it fails on the committed r-restart-1 close hook (`design/lite-code/atm-bd-task-hook.py` at the current head), which is the fixture for "stack pushed into close".

## C. Guardrails against review-driven growth

The gate in section B runs on every push to a phase-bo sprint branch, in CI and as the plan-anchor pre-commit. A PR cannot merge with a failing gate. Reviews cannot waive it.

- **ANC-070 Line ceiling.** Each hook file ≤ 100 lines including header and docstring. Check: `wc -l`. Fail fixture: 101 lines. This is the cheapest creep detector there is; a stack claim does not fit.
- **ANC-071 Spec is owned.** `intent.md` and `hooks/*.md` change only in a commit authored by Rand. Check: `git log --format=%ae -- docs/plans/phase-bo/intent.md docs/plans/phase-bo/hooks/` shows only rand@randlee.com. Fail fixture: a commit from fenix editing a Transitions table. If fenix or a reviewer thinks the spec is wrong, the output is a question to Rand, never an edit.
- **ANC-072 Review findings bind to the spec, not the code.** A review finding that would add a write, an arm, a subprocess or a guard to a hook is out of scope unless it cites the `hooks/*.md` row it implements. The reply template is one line: "Not in <hook>.md; file against the spec (ANC-071)." Check: ANC-064/065/066/070 fail any such change regardless of what the review said. Fail fixture: solar round asks for a retry path; fenix adds an arm; gate fails on rows ≠ arms.
- **ANC-073 Every anchor check is mechanical.** A new anchor whose check is prose ("is simple", "is maintainable") is rejected by plan-anchor. Check: each anchor's `check` names a command (grep, wc, ast, pyright, the gate) or a Jev fixture pair. Fail fixture: "Jev judges the hook is reasonable."

Ordering: land section A anchors and the section B gate **before** bo-10. A gate that arrives after the code is a gate the code was never held to.

## What Rand does
Approves `intent.md` and `hooks/*.md` as the owned spec (one commit). Answers questions fenix raises under ANC-071. Reviews the bo-10 diff. Nothing else.
