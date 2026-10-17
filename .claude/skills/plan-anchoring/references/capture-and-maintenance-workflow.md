---
version: 0.6.0
---

# Capture and maintenance workflow

## Concepts

| Concept | ID | What it is | Where it comes from |
| --- | --- | --- | --- |
| Intent | none | One-page "why" for the plan; frames every question, never judged | Written during planning |
| Anchor | ANC-NNN | One testable requirement written from something the user said; the exact words are kept as its `quote` | The user, via the orchestrator, approved by the user |
| Requirement | REQ-* | A numbered technical requirement | Existing register |
| ADR | ADR-* | A numbered architectural decision record | Existing register |

All three ID'd kinds become Jev questions and are judged the same way. Details: `references/anchor-document.md`.

## Layout

```
doc/plan/phase-current/
├── intent.md            # the overview
├── anchors.json         # record of truth: every anchor with provenance, status, beads, fixtures
├── anchors.md           # rendered from anchors.json; never hand-edited
└── anchor-runs/         # one JSON per gate run, dry runs included; versioned with the plan for diffs
```

Beads hold the questions: one anchor bead per fact, with a plan-questions child and a dev-questions child (`references/bead-layout.md`).

## Workflow

### A. Capture anchors while planning (main session, interactive)

Read `references/anchor-capture.md`. Watch for directives about how the work must be done; a phase needs five to fifteen, not one per sentence. For each candidate, write one testable requirement (`statement`) and keep the user's exact words as the `quote`. Before proposing, run `python3 "$S" rubric --statement "<candidate>" --quote "<words>" --scope "<scope>"`: deterministic lint checks requirement form, while two Jev confirmations judge one constraint, observable behavior, fidelity, scope, and self-contained conditions. A third call occurs only on choice disagreement. Fix `needs_work`; clarify `inconclusive` or `unstable` rather than treating either as a pass. Propose both texts together, check `anchors.md` for duplicates, and record only when the user confirms or says "lock this". The statement is never the quote; `add` refuses that, and `add --rubric` refuses any result except `testable`.

### B. Lock and record

```bash
mkdir -p doc/plan/phase-current
[ -f doc/plan/phase-current/intent.md ] || cp "$(dirname "$S")/../assets/templates/intent.md" doc/plan/phase-current/intent.md
python3 "$S" add --anchors "$A" --quote "<the user's exact words>" \
  --statement "<one testable requirement: subject, must/must not, action, scope>" \
  --kind directive|prohibition|structural --stated-by "<name>" --context "planning session" --summary "<short label>" \
  --scope "<bead-level signals: adds a CLI subcommand, edits Cargo.toml, touches crates/x>" \
  --deterministic-check "<diff, git log, grep or test that proves it literally>" --status locked   # first add creates anchors.json
  # add runs the testability lint and refuses near-duplicates (ANCHOR.DUPLICATE_CANDIDATE); see references/anchor-capture.md
python3 "$S" update --anchors "$A" --id ANC-001 --fixture-pass "<a sentence a bead would say if it satisfied this>" \
  --fixture-fail "<a sentence that would violate it>"                                              # draft, then get the user's approval
python3 "$S" validate --anchors "$A" && python3 "$S" render --anchors "$A"
```

Then create the bead tree and record its IDs with `set-beads` (`references/bead-layout.md`). Requirements and ADRs governing this plan are added the same way with `--source-type REQ|ADR --source-ref <ID>`; they keep their register ID. Fill in `intent.md` if just created. Before the first gate run, every locked anchor must be `lint` clean, have approved fixtures, and pass `check-fixtures` with the default questions; otherwise tune first (B2). Periodically run `dedupe --anchors "$A"` and fold duplicates (`retire --replaced-by`).

### F. Maintain

`python3 "$S" promote --anchors "$A" --id ANC-012 --adr ADR-0007` promotes a durable anchor, keeping the ID. A changed directive is a new anchor; retire the old with `python3 "$S" retire --anchors "$A" --id ANC-012 --replaced-by ANC-020 --reason "<why>"`. Scope, summary, fixtures and deterministic checks change through `update`. Re-render after every change. Lifecycle: `references/anchor-document.md`.

<!-- The workflow text above is preserved verbatim from the 0.4.3 entrypoint. -->
