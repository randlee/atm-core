---
name: plan-anchoring
description: Capture the facts a user states while planning as numbered anchors (ANC-001...), one clear, testable requirement each, with the user's verbatim words kept as provenance, alongside existing requirements (REQ) and ADRs. Turn them into Jev-judged questions, tune and test the questions per anchor, and enforce them at two gates with a triage-apply-rerun loop. The planning gate checks each plan bead still says what the user said; the development gate checks the delivered sprint work followed it. Use when planning a phase or sprint with beads, when the user says "anchor this", "lock this", "don't let this drift", or "remember I said", when re-opening a plan after context compression, or when reviewing a sprint against its plan. Not for general code review, and not a replacement for plan hardening.
metadata:
  version: "0.9.2"
---

# Plan Anchoring

Facts stated during planning get compressed out of context, ceremony accumulates, unapproved work creeps in, and plans and code drift from what was said. This skill makes each stated fact durable: the user's words are kept verbatim; the orchestrator writes each as one testable requirement the user approves; the requirement becomes orthogonal Jev questions, tuned per anchor; and the plan and the delivered work are judged against it in a loop of run, triage, fix, re-run.

This file is the table of contents; read the reference for the step you are on.

## Step 1 - Verify dependencies

Run before any capture, delegation, or gate. On a missing dependency, surface the gap and stop.

```bash
which python3 && python3 --version          # 3.9 or newer
python3 -c 'import yaml; print("PyYAML", yaml.__version__)'   # anchor_tracker.py only; python3 -m pip install pyyaml
which jq && jq --version                    # gate agents parse question overrides with jq
T="<this skill's directory>/scripts/anchor_tracker.py"
python3 "$T" resolve > /tmp/anchor-tracker.json; jq '{success, error: .error.code}' /tmp/anchor-tracker.json
ANCHOR_TRACKER="$(jq -r '.data.tracker // empty' /tmp/anchor-tracker.json)"
which "$ANCHOR_TRACKER" && "$ANCHOR_TRACKER" --version
which sc-compose && sc-compose --version    # 1.7.0 or newer
test -n "$TYPESAFE_API_KEY" && echo "TYPESAFE_API_KEY set (preferred)" || echo "TYPESAFE_API_KEY absent; live Jev may use the documented platform credential store"
python3 "<this skill's directory>/scripts/plan_anchor_settings.py" show | jq '.data.allow_remote_judge'   # true, false, or null (never asked)
```

`allow_remote_judge` in the committed `.sc/plan-anchor/settings.yaml` is the user's answer to whether plan text may be sent to TypeSafe (Jev). `true`: continue. `false`: every live Jev call is refused with `POLICY.REMOTE_JUDGE_DISABLED`; tell the user and do only offline work. `null` (absent): ask the user once, record the answer with `python3 "<this skill's directory>/scripts/plan_anchor_settings.py" set allow_remote_judge true|false`, commit the file, then continue; never ask again in later sessions. Never infer consent from existing run files, committed evidence or past runs, and never write the setting without the user's answer; subagents never write it.

`anchor_tracker.py resolve` confirms `ANCHOR_TRACKER` for anchor storage, independently of whether plans are Markdown or tracker documents. This install is rendered for `bd`: `.sc/plan-anchor/settings.yaml` sets `anchor_tracker: bd`, and `resolve` checks it against disk (`.beads/` must be Dolt-backed). `success: false` (exit 2) is a typed error (`TRACKER.MISMATCH`, `TRACKER.DOLT_REQUIRED`, `TRACKER.CLI_MISSING`, `SETTINGS.*`, `DEPENDENCY.PYYAML`): surface it and stop; never pick another tracker by hand. If `resolve` selects anything but `bd`, stop: the repository changed trackers and the plugin installer must be rerun with `--tracker` (`references/installation-and-troubleshooting.md`). `resolve` is read-only. Never run `bd init`: bd is supported only where a Dolt-backed `.beads/` already exists. Pass the resolved value as `anchor_tracker` to both gate reviewers, which resolve it again and reject a mismatch. Both take the sprint text the caller already has, as `item_file`; nothing rediscovers it. Tracker command examples use `bd`.

If `resolve` reports `TRACKER.CLI_MISSING` (its `data.tracker` names the selected tracker, so `ANCHOR_TRACKER` is still set), or `sc-compose` is missing, Claude Code's bash may not share your shell's PATH. Check common locations:

```bash
for p in "/opt/homebrew/bin/$ANCHOR_TRACKER" "$HOME/go/bin/$ANCHOR_TRACKER" "$HOME/.local/bin/$ANCHOR_TRACKER" "/usr/local/bin/$ANCHOR_TRACKER"; do
  [ -x "$p" ] && echo "Found at: $p" && break
done
for p in "/opt/homebrew/bin/sc-compose" "$HOME/.local/bin/sc-compose" "/usr/local/bin/sc-compose"; do
  [ -x "$p" ] && echo "Found at: $p" && break
done
```

If found off PATH, `export PATH="<dir>:$PATH"` for the session and rerun `resolve`. If anything is missing, read `references/installation-and-troubleshooting.md`. This skill requires Jev: capture runs the live rubric and question tuning and both gates make live judgments, so the normal workflow requires the API key and `allow_remote_judge: true` (`references/jev-api.md`). Offline `--dry-run`, fixture-answer tests, lint, shape, rendering, and record maintenance remain available for development, but they are not a substitute for the Jev-backed workflow.

Then resolve the script once. It sits next to this file:

```bash
S="<this skill's directory>/scripts/jev_judge.py"   # absolute: <install folder>/.<harness>/skills/plan-anchoring/scripts/jev_judge.py
P="$(python3 "<this skill's directory>/scripts/plan_anchor_settings.py" show | jq -r '.data.plan_dir // empty')"   # e.g. docs/plans/phase-b
A="$P/anchors.json"
python3 "$S" --help
```

`P` is the current phase's plan folder, `docs/plans/phase-<current>`, where `<current>` is the phase id (`docs/plans/phase-b`); it holds `intent.md`, `anchors.json`, `anchors.md` and `anchor-runs/`. It is `plan_dir` in the committed `.sc/plan-anchor/settings.yaml`. Empty (never set): ask the user which phase is current, record it with `python3 "<this skill's directory>/scripts/plan_anchor_settings.py" set plan_dir docs/plans/phase-<id>`, commit the file, then continue. When the phase changes, set it again. Never write anchors to a folder the user has not named.

Every command below uses `$S`, `$P` and `$A`; every agent invocation passes `$S` as `script` and `$A` as `anchors_path`.

## Select the workflow

After dependency verification, read only the row for the current operation.
The workflow retains the tested language from this file and points to deeper
contracts only when that operation needs them.

| Operation | Read |
| --- | --- |
| Capture, lock, or maintain anchors and requirements | `references/capture-and-maintenance-workflow.md` |
| Calibrate the rubric or tune anchor questions | `references/question-tuning-workflow.md` |
| Review a Markdown or `bd` plan | `references/planning-review-workflow.md` |
| Review delivered sprint work | `references/development-review-workflow.md` |

Do not preload the other workflow references. Open only additional references
named by the selected workflow when their detail is needed.

## Agent Delegation

When the selected workflow delegates, read
`references/agent-delegation-workflow.md` before invoking an agent. Capture and
locking do not load that reference.

## Design positions

- **Granularity**: one testable constraint per anchor; a compound statement becomes several anchors, each quoting the sentence carrying its constraint.
- **Capture**: the orchestrator proposes candidates it detects; nothing is locked until the user confirms or says "lock this".
- **Fast-model cross-check**: not in this version. Jev's per-question confidence drives the `underspecified` verdict instead.
- **Relationship to plan hardening**: standalone and composable; run the planning gate after hardening.
- **Data retention**: fail closed. Every live Jev call is refused unless the committed `.sc/plan-anchor/settings.yaml` sets `allow_remote_judge: true`; the user decides it once per repository, and `--dry-run` writes exactly what would leave the machine.
- **Fixture authorship**: the skill drafts one pass and one fail fixture per anchor at lock time; the user approves them. `check-fixtures` sends them to Jev (same setting) to confirm the wording lands them; `--answers-file` fixtures exercise the gating offline in CI.
- **Not relevant to one item**: `exclude` is the one way to say an anchor does not apply to a specific plan item: `python3 "$S" exclude --anchors "$A" --id ANC-011 --item bi-2 --reason "<one sentence>"`. `judge` then skips that cell without calling Jev and reports it `excluded` with the reason; it passes the gate. No approval, no frozen expectation, no version bump, and other runs stay valid; it is not QA-verified, so the reason must say why. Use it when the anchor's scope is right and the item simply does not touch it; when the scope itself is wrong, fix the scope (`references/triage-and-iteration.md`). Do not drop the anchor from `--anchor` or override its relevance question instead; neither is recorded.
- **Question-tuning independence**: the planner cannot tune. An isolated evaluator (Claude: Opus; Codex: Sol; Cursor: Grok at high effort, version left off) derives and freezes its oracle from approved intent and anchors before seeing Jev answers, then iterates wording against that oracle without regard to whether the current plan passes. A live gate failure alone never authorizes an override or scope change. The main session records only an override accompanied by the frozen oracle, digest, acceptance table, and run IDs.

## Hard stops

- An anchor without a verbatim `quote` has no provenance; one whose statement is just the quote is not testable. Both are refused.
- Never tune a question or reword a statement to make one bead pass. Fixtures and a known-bad bead must still fail.
- Never tune or rescope from a live failure alone. Require a frozen, user-approved fixture or expected verdict that independently demonstrates the false result; scope changes require user approval and invalidate prior gate runs for that anchor. An `exclude` is not a rescope and needs neither.
- A failing-to-passing plan edit that grows the composed item is a hard stop unless each added passage is traced to a locked ANC/REQ/ADR that predates the failed run.
- Never split a bead, trim its text, or raise a size cap to pass `PLAN.TOO_LARGE`. Triage unapproved work, then prose and ceremony, then split.
- Never send more than one bead or one work description per Jev request, and never the whole plan.
- Never compute coverage, counts, or "satisfied anywhere" by asking Jev. The script does it.
- Missing the selected anchor tracker, `python3`, PyYAML, or API key, `allow_remote_judge` not `true`, or an `anchor_tracker.py resolve` error: surface and stop; do not degrade silently.
- Three consecutive gate runs on the same bead with the same failing set: escalate to the user rather than editing the plan again.

Maintainers only: the complete resource index is in
`references/reference-catalog.md`; ordinary workflow execution does not load it.
