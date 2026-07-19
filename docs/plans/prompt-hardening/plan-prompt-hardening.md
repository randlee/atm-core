---
title: Prompt Hardening Plan
status: complete
branch: feature/prompt-hardening
worktree: ../atm-core-worktrees/feature/prompt-hardening
---

# Goal

Harden prompt and template surfaces against false closure, stale citations, and
ambiguous reporting without adding phase-specific gate logic to general prompts.

# In Scope

- `.claude/skills/codex-orchestration/dev-template.xml.j2`
- `.claude/skills/codex-orchestration/qa-template.xml.j2`
- `.claude/skills/codex-orchestration/review-template.xml.j2`
- `.claude/agents/quality-mgr.md`
- `.claude/skills/codex-orchestration/ruthless-boundary-qa-assignment.json.j2`

# Proposed Changes

1. Dev completion reports require deliverable inventory per claimed acceptance
   item: file, symbol or code path, live line, short change note.
2. Deletion or cleanup sprints require before/after targeted symbol or
   code-path inventory; net LOC is secondary evidence only.
3. QA and review reports must re-verify every cited `file:line` against the
   current branch/worktree before reporting it.
4. Missing or stale evidence is reported as a finding, not inferred as closure.
5. QA dispatch requires ruthless-boundary-qa to run a workspace-wide duplicate
   sweep for every constant, function, or type touched by the sprint diff.

# Out Of Scope

- phase-specific gate mechanics
- CI wiring
- runtime or product code changes
- reviewer-set policy changes beyond the citation rule above

# Verification

- inspect each edited prompt/template for the required inventory or
  citation-verification rule
- render/read the edited files directly and confirm wording is concise
- confirm the scope stays limited to prompt/template/agent text only
