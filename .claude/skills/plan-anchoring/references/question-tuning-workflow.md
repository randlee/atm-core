---
version: 0.6.1
---

# Question-tuning workflow

### B2. Tune and test the questions per anchor

Read `references/question-tuning.md`. After the first draft and approved
INTENT/ANC packet exist, launch one named `anchor-tuning` planner subagent
(Claude: Opus; Codex: Sol; Cursor: `grok[effort=high]`, version left off) and continue authoring concurrently. Cursor launches it with the Task tool and does not pass a model argument. Render its one
fenced request from `templates/anchor-tuning-tune-request.md.j2`; never supply a
desired plan verdict. The tuner freezes an oracle, evaluates a larger candidate
pool through parallel Jev calls, and returns the smallest semantically exact,
stable role-complete set plus canonical history. Validate its schema and
`anchor_tuning_contract.py` result (one JSON envelope: exit 0 with
`data.valid: true`, exit 2 with a typed `error`) before writing the selected questions to the
`bd` or `br` question record. Leaf review waits for current protocol seals. On
ambiguous authority, present the tuner's clarity-only recommendation to the
user; the planner never tunes questions itself.

Exclusions are not a tuning outcome. When an anchor's scope is right and one
item simply does not touch it, record `exclude` (`anchor-document.md`) instead
of tuning its questions or scope. The tuner's `authority.anchors.records` carry
no `exclude` field, so an exclusion changes no authority digest and no seal.

Before this per-anchor tuning, a changed or unsealed testability rubric must be
calibrated by the same higher-reasoning agent with operation `testability`, rendered
from `templates/anchor-tuning-testability-request.md.j2`. Its frozen
plan-independent corpus is the authority: two confident agreeing answers are
required per accepted cell, with a third call only to resolve disagreement.
Requirement form remains deterministic lint and dedupe compares statements,
never shared provenance quotes.
