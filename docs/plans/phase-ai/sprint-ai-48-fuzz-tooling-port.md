---
title: AI.48 fuzz tooling port
status: planned
branch: feature/pAI-s48-fuzz-tooling-port
recommended_agent: Cipher-311d
recommended_model: fast
execution_mode: parallel
target: integrate/phase-ai-31-33
depends_on: AI.47
---

# AI.48 — Fuzz tooling port

## Goal

Port the bounded sc-compose fuzz coordinator/probe workflow and expose it as
`just fuzz`; it produces validated machine results but no HTML report.

## Source Material

Source: `randlee/sc-compose`.

- `.claude/skills/adversarial-fuzzing/SKILL.md`
- `.claude/agents/sc-adversarial-fuzz-coordinator.md`
- `.claude/agents/sc-adversarial-fuzz-probe.md`
- `.claude/skills/html-report/templates/fuzz-run-report.html.j2`
- `.claude/skills/html-report/templates/fuzz-run-agent.xhtml.j2`
- `.claude/skills/html-report/fuzz-run-agent-contract.md`

AI.48 copies the skill and agents; AI.50 owns the listed report templates.

## Exact Targets

- `Justfile` `fuzz` recipe and runner/tests
- `.claude/skills/adversarial-fuzzing/SKILL.md`
- `.claude/agents/sc-adversarial-fuzz-coordinator.md`
- `.claude/agents/sc-adversarial-fuzz-probe.md`

## Deliverables

1. Copy the named sc-compose skill and coordinator/probe agents verbatim.
2. Add `just fuzz` with one fenced JSON campaign contract, safe approved
   worktree validation, four-worker cap, deterministic seeds/correlation IDs,
   and structured non-lossy worker results.
3. Keep report HTML/XHTML generation out of this sprint; AI.50 owns it.

## Required Validation

- fixture campaigns for success, timeout, malformed result, and unsafe path
- `just fuzz --dry-run`
- `just lint`

## Acceptance Criteria

`just fuzz` produces bounded, deterministic, schema-valid worker results and
does not edit product code or create report HTML.

## Non-goals

No real campaign, report rendering, Pages work, or parser modification.
