---
version: 0.6.1
---

# Agent delegation workflow

## Agent Delegation

Capture (A) and locking (B) stay in the main session. Question tuning (B2) and the gates (C, D) run in isolated agents that return fenced JSON; the tuner runs in the background. The skill presents results and makes every write to beads and the record.

Before invoking any bundled agent, resolve it once and use the single-envelope
boundary in `references/agent-runner-contract.md`. The planner owns trust
resolution and attestation. Leaf agents receive the immutable descriptor; they
do not search installation paths or repeat registry/frontmatter validation.

| Operation | Agent | Inputs | Returns |
| --- | --- | --- | --- |
| Planning gate, one complete sprint | `anchor-plan-review` | `script`, `item_id`, `item_path`, `anchor_tracker` (`bd` or `br`, from `anchor_tracker.py resolve`), `anchor_ids` or `all`, `anchors_path`, optional `run_dir`, `threshold`, `dry_run`, `correlation_id` | fenced JSON: run path, pass flag, per-anchor verdicts, report markdown |
| Development gate, one sprint | `anchor-sprint-review` | `script`, `sprint_id`, `evidence` paths or `pr_number`, `anchor_ids` or `all`, `anchors_path`, optional `run_dir`, `threshold`, `dry_run`, `correlation_id` | fenced JSON: run path, pass flag, per-anchor verdicts, deterministic checks to pair, report markdown |
| Independent rubric calibration, question tuning, and evidence triage (Opus/Sol/Grok high) | `anchor-tuning` | one strict fenced JSON object rendered from the checked-in testability, tune, or triage template, including resolved runtime, authority packet or frozen corpus, protocol/history where applicable, and correlation ID | strict Standard-envelope fenced JSON rendered from the result template and validated by schema plus `anchor_tuning_contract.py` |

In Claude Code, invoke the plugin's harness-specific file in `.claude/agents/` with the **Agent** tool, never the Task tool; the manifest registers gate agents with `model: sonnet` and the independent tuner with `model: opus`. In Codex, read the matching plugin-owned definition in `.codex/agents/` and launch it with the Agent tool; gate agents specify `model: luna` and the tuner `model: sol`. Codex plugins discover the skill but do not auto-register these agent profiles, so the skill owns this explicit launch step. Example gate input: "Invoke `anchor-plan-review` with script `$S`, item_id A1, item_path doc/plan/phase-current/A1.descriptor.json, anchor_tracker br, anchor_ids all, anchors_path doc/plan/phase-current/anchors.json, correlation_id A1." In Cursor, invoke the registered subagent with the **Task** tool and do not pass a model argument. Gate agents are `anchor-plan-review` and `anchor-sprint-review` with `model: composer`. The tuner is `anchor-tuning` with `model: grok[effort=high]` (Grok at high effort, version left off) and `is_background: true`. If those subagent types are not registered, stop and tell the user to install the `sc-plan-anchor` Cursor plugin. Question tuning does not fall back to the planner, and a live gate does not run in the parent chat.

For planning review, persist each discovered sprint descriptor (one element of `data.sprints` from `plan_source.py`, not the envelope) as its own repository-local JSON file and pass its path as `item_path`. Preserve its normalized text and digest exactly. Take `anchor_tracker` from `anchor_tracker.py resolve` (the `data.tracker` field), independently of the plan source: a Markdown plan with `br` anchors does not need a planning bead or `bd`. The reviewer resolves it again from the same repository and returns `VALIDATION.TRACKER` if the value passed differs. This applies to the planning gate only; `anchor-sprint-review` is unchanged and still uses `bd`. Verify any tracker copy parity before dispatch; the leaf never reconstructs the item from title/acceptance fields.

Parallel runs: at most 3 agents at once, `correlation_id` set to the bead ID, 300 s timeout per agent (Jev takes seconds; the budget is the agent's own turns). An agent that times out or returns unfenced or malformed JSON has produced no result: re-dispatch that bead once, passing as `run` any run file it already wrote, so Jev is not asked again. Only when the retry also fails, record `{"success": false, "canceled": true, "aborted_by": "timeout", "retried": true, "error": {"code": "EXECUTION.TIMEOUT"}}`. Aggregate in bead-ID order; never drop a failed bead. Agents never retry the script; it already makes three attempts per anchor and never shortens planning items.
