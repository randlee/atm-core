---
name: report-format
version: 0.6.1
description: The provenance-quoting report format for gate results, what the planning agent or quality manager should do with each verdict, and where reports are posted.
---

# Report format

Every failing verdict names the anchor and quotes the user's words behind it. `python3 "$S" report --run <file> --format md` renders it; nothing is written by hand.

## Shape

```markdown
## Anchor gate: plan · bd-42 - Sprint 2 runtime

Run `plan-20261009T183000Z-bd-42`, model `jev-1.13.0`, threshold 0.6. Counts: not_addressed 1, satisfied 3, not_applicable 9.

> **bd-42 · ANC-012 · not addressed** Anchor: The runtime crates are created by copying the legacy repo's runtime crates and deleting the legacy-specific modules; no runtime crate is written from scratch. You said (planning session, Oct 9): "copy the libraries from the legacy repo as the starting point and remove the legacy-specific code" This bead does not carry the directive forward.

> **bd-42 · REQ-014 · contradicted** Anchor: All public CLI commands must support --json output with a stable schema. This bead proposes something the anchor rules out.

> **bd-42 · ANC-003 · error** Anchor: Sprint 2 adds no new [dependencies] entry to any Cargo.toml. You said (planning session, Oct 9): "no new dependency for this" Judging failed; see error.
>   Error JEV.CONTEXT_EXCEEDED: item could not be shortened below 2000 characters
```

Line anatomy: `item · anchor · verdict`; `Anchor:` with the judged statement; provenance, `You said (<context>, <date>): "<quote>"` for ANC or `<ID> (<register>) says: "<quote>"` for REQ and ADR when the register text differs from the statement; then a one-sentence explanation that depends only on the verdict, because Jev does not explain itself. Error rows add the code and message. Underspecified rows add one `Low:` line per low answer (question, choice, confidence), which tells the planner where the bead is weak. Satisfied and not-applicable rows are omitted unless `--all` is passed; development-gate rows add the paired deterministic check where one exists.

## What to do with each verdict

Before acting on a failing verdict, re-read the sent text (the composed item file) where the anchor governs; Jev read it literally. A failing verdict or low confidence is a plan-quality signal for the planner; the usual fix is to simplify the bead. Work the change cannot succeed without stays in the plan even when Jev flags it as extra: add one line to the bead saying why it is required, and list the cell with that line in the planning-complete summary.

| Verdict | Planning gate (planning agent) | Development gate (quality manager) |
| --- | --- | --- |
| `contradicted` | Rewrite the bead's approach to match the directive, or escalate if the user has changed their mind (then `retire` the anchor and lock a new one) | Block acceptance; the work changes or the user retires the anchor explicitly |
| `not_addressed` | Add the directive's specific content to the bead: starting point, constraint, step | Block acceptance until the sprint report shows the directive was followed; re-run |
| `asserted_only` | n/a at this gate | Ask for evidence (paths, diff, command, test) or run the paired `deterministic_check` and record it |
| `underspecified` | Read `low_questions` and the agent's `to_fix`, and fix the bead there, preferring cuts to additions. Only when the cell is low on several beads that plainly comply: sharpen the anchor's `scope` (`update --scope`) or override its questions | Same; a vague sprint report is a reporting defect, not an anchor defect |
| `satisfied` | Nothing | Run the paired deterministic check if any |
| `not_applicable` | Nothing. If the anchor should have applied, its `scope` is too narrow | Same |
| `excluded` | Nothing; the report always lists it with the exclusion's reason. Remove the exclusion if the reason is wrong | Same |
| `error` | `JEV.CONTEXT_EXCEEDED`: shorten the item; transport codes: retry later, the script already tried three times | Same |

## Where reports go

- Planning gate: comment on the judged planning bead:
  ```bash
  python3 "$S" report --run <run.json> --format md > "$W/report.md" && bd comment <bead> --file "$W/report.md"
  ```
- Development gate: comment on the sprint bead and, when there is a PR, on the PR, with the deterministic check outcomes beneath.
- Both: the run JSON stays in `doc/plan/phase-current/anchor-runs/` and is committed with the plan.

The skill posts in the main session; agents return JSON and never write to beads or PRs.

## Coverage report

`coverage --format md` renders the anchor x item matrix with a Covered column and a list of uncovered anchors. Include it at the end of a planning-gate pass so the user sees the whole plan's state.
