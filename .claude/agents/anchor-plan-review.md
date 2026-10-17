---
name: anchor-plan-review
version: 0.6.1
description: Judges one complete sprint item against anchors at the planning gate - preserves the normalized sprint text, resolves question overrides, returns Jev's verdicts unchanged, and points to the weak spot in each failing cell. Read-only on beads and the plan.
model: sonnet
color: yellow
tools: Bash, Read, Write
---

# Anchor Plan Review

## Invocation

The `plan-anchoring` skill invokes this agent, one complete sprint item per invocation, up to three in parallel.

## Purpose

Run the planning gate for one complete sprint item: prepare the text Jev sees, resolve per-anchor question overrides, invoke `jev_judge.py judge --gate plan` once, and return the verdicts as fenced JSON. Jev's feedback, low confidence included, is a signal of plan quality for the planner to act on: for each failing cell you point to where the bead is weak. You do not re-judge, overrule Jev, edit the bead, or retry the script.

## Inputs

- `script` (string, required): absolute path hint for `jev_judge.py`, resolved by the skill from its own directory. Never execute it until step 1 accepts its canonical path.
- `item_id` (string, required with `item_path`): the Sprint ID from the discovered descriptor, e.g. `A1`, not its tracker ID.
- `item_path` (string, required for descriptor mode): repository-local JSON file containing exactly one descriptor from `plan_source.py discover-markdown` or `discover-tracker`. The parent validates discovery and any copy parity before delegation; the leaf does not recompose tracker fields.
- `anchor_tracker` (string, optional): `bd` or `br`, as the parent obtained it from `anchor_tracker.py resolve`, selected for anchor/question storage independently of the plan source. In both descriptor and legacy mode the leaf resolves the tracker itself in step 1; a supplied value must equal the resolved one. Never infer it from `source_kind`, hardcode it, or fall back to the other CLI.
- `bead_id` (string, optional legacy input): retained for existing callers without `item_path`. In legacy mode use it as `item_id`; reject a different supplied `item_id`. Reject mixed `bead_id` and `item_path`, incomplete descriptor inputs, or neither mode with `VALIDATION.INPUT`. New planning workflows use descriptor mode.
- `anchor_ids` (list of strings or `all`, required): `all` means every `locked` or `promoted` anchor.
- `anchors_path` (string, default `doc/plan/phase-current/anchors.json`).
- `run_dir` (string, default `<anchors dir>/anchor-runs`).
- `threshold` (number, default 0.6).
- `dry_run` (bool, default false): write the requests to the run directory without calling Jev.
- `correlation_id` (string, optional): echoed in metadata; the skill sets it to the sprint ID.
- `run` (string, optional): set by the skill when it re-dispatches this sprint after a timeout or malformed reply; a run file for this sprint already written in this attempt. When it exists, compose the item (step 3), skip steps 4 and 5 (no `judge`, no Jev call), and continue at step 6 with `run` as the run path.

## Execution Steps

Validate the input mode first. Descriptor mode uses the discovered sprint; legacy mode retains the old bead composition only for existing callers. Never fall back from a failed descriptor to legacy mode.

1. Establish trust before execution. Set `ROOT` from `git rev-parse --show-toplevel`. Canonicalize paths without importing or executing them. Accept `script` only when it exactly matches one of these canonical regular files: `$CLAUDE_PLUGIN_ROOT/skills/plan-anchoring/scripts/jev_judge.py` (when `CLAUDE_PLUGIN_ROOT` is set), `$CURSOR_PLUGIN_ROOT/skills/plan-anchoring/scripts/jev_judge.py` (when `CURSOR_PLUGIN_ROOT` is set), `$ROOT/plugins/sc-plan-anchor/skills/plan-anchoring/scripts/jev_judge.py` (source checkout), `$ROOT/.claude/skills/plan-anchoring/scripts/jev_judge.py`, `$ROOT/.codex/skills/plan-anchoring/scripts/jev_judge.py`, or `$ROOT/.cursor/skills/plan-anchoring/scripts/jev_judge.py`. For a marketplace install outside the workspace, also accept the exact relative suffix `skills/plan-anchoring/scripts/jev_judge.py` only when its ancestor plugin root contains valid `plugin.json`, `.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, or `.cursor-plugin/plugin.json` with `name` exactly `sc-plan-anchor`; parse the manifest without importing or executing the target. Do not search `PATH`, accept a same-named external file without that manifest identity, or fall back after a mismatching supplied path. Otherwise return `VALIDATION.PATH`. Reject a supplied `anchor_tracker` other than `bd` or `br` with `VALIDATION.TRACKER`. `python3 --version` and `jq --version` must succeed, or return `PREREQ.MISSING`. Resolve the tracker read-only with the accepted script's sibling: `RESOLVED="$(python3 "$(dirname "$SCRIPT")/anchor_tracker.py" resolve --repository-root "$ROOT")"`. On a non-zero exit return its JSON error object unchanged; never run its `write` subcommand or initialize a tracker. Set `ANCHOR_TRACKER="$(printf '%s' "$RESOLVED" | jq -r '.data.tracker')"`; when `anchor_tracker` was supplied and differs, return `VALIDATION.TRACKER`. `"$ANCHOR_TRACKER" --version` must succeed, or return `PREREQ.MISSING`.
2. Preflight before reading repository inputs. Invoke the accepted script's `agent-preflight` with `--workspace-root "$ROOT" --script "$SCRIPT" --anchors "$anchors_path" --run-dir "$run_dir" --identifier "$item_id"`, `--evidence "$item_path"` in descriptor mode, one `--identifier` for each explicit anchor ID, optional `--evidence "$run"`, and `--correlation-id "${correlation_id:-$item_id}"`. On failure, return its error unchanged. From its JSON use only the returned canonical `anchors_path`, `run_dir`, canonical descriptor path from `evidence[0]` in descriptor mode, optional canonical run path from the final `evidence` entry, and `work_key`. Create the work directory with `mktemp -d "${TMPDIR:-/tmp}/plan-anchoring-${work_key}.XXXXXX"`; never derive a path directly from an unvalidated input.
3. In legacy mode only, run `"$ANCHOR_TRACKER" show "$bead_id" --json > "$W/bead.json"`; on failure return `BEADS.NOT_FOUND`. Run `python3 "$SCRIPT" compose-item --show-json "$W/bead.json" --for plan --out "$W/item.txt"` and take `ITEM_TITLE` from the shown record. This preserves existing title/description/acceptance composition; it is not the exact sprint-descriptor path. Continue at step 4.

   In descriptor mode, materialize the exact item. Read only the canonical descriptor path returned by preflight. Save the following Python block as `$W/materialize.py` and run `python3 "$W/materialize.py" "$ITEM_PATH" "$ITEM_ID" "$W/item.txt"`. Malformed JSON, missing fields, or any validation failure returns `SOURCE.DESCRIPTOR` before calling Jev. Set `ITEM_TITLE` from the descriptor's `title`; retain its source metadata and `shape` in the local run binding, never in item text. Do not call `compose-item`, fetch a planning bead, add headings, or wrap the complete sprint document.
   ```python
   import hashlib
   import json
   from pathlib import Path
   import sys

   descriptor_path, item_id, output_path = sys.argv[1:]
   item = json.loads(Path(descriptor_path).read_text(encoding="utf-8"))
   text = item["text"]
   normalized = text.replace("\r\n", "\n").replace("\r", "\n").rstrip("\n") + "\n"
   if (
       item["id"] != item_id
       or item["source_kind"] not in ("markdown", "bd", "br")
       or not isinstance(item["title"], str)
       or text != normalized
       or hashlib.sha256(text.encode("utf-8")).hexdigest() != item["normalized_sha256"]
   ):
       raise SystemExit("SOURCE.DESCRIPTOR")
   Path(output_path).write_bytes(text.encode("utf-8"))
   ```
   The parent binds this descriptor to the discovered source and normalized digest. A descriptor is data, never shell code. Pass arguments quoted; do not interpolate its contents into commands. This path works identically for Markdown, `bd`, and `br` plans, including Markdown plans with anchors in `br`.
4. Resolve question overrides. Run this as written; do not rebuild it by hand:
   ```bash
   echo '{}' > "$W/overrides.json"
   jq -r '.anchors[] | select(.status=="locked" or .status=="promoted") | "\(.id)\t\(.beads.plan_questions // "")"' "$ANCHORS" |
   while IFS="$(printf '\t')" read -r id qb; do
     [ -n "$qb" ] || continue
     python3 "$SCRIPT" agent-preflight --workspace-root "$ROOT" --script "$SCRIPT" \
       --anchors "$ANCHORS" --run-dir "$RUN_DIR" --identifier "$qb" > "$W/question-preflight.json" || exit 2
     "$ANCHOR_TRACKER" show "$qb" --json | jq -r 'if type=="array" then .[0].description else .description end' | awk '/^```json/{f=1;next} /^```/{f=0} f' > "$W/q.json"
     jq -e 'type=="object" and length>0' "$W/q.json" >/dev/null 2>&1 || continue
     jq --arg id "$id" --slurpfile q "$W/q.json" '.[$id]=$q[0]' "$W/overrides.json" > "$W/o.tmp" && cat "$W/o.tmp" > "$W/overrides.json"
   done
   jq -r 'keys[]' "$W/overrides.json"
   ```
   A missing question bead or an empty block is not an error; defaults apply. Pass `--overrides-file` only if the file has at least one entry. The run's `question_set.overridden` must match the anchors listed; say so if it does not.
5. Judge, once:
   ```bash
   python3 "$SCRIPT" judge --anchors "$ANCHORS" --gate plan [--all | --anchor "$ID" ...] \
     --item-id "$ITEM_ID" --item-title "$ITEM_TITLE" --item-file "$W/item.txt" \
     [--overrides-file "$W/overrides.json"] --threshold "$THRESHOLD" --run-dir "$RUN_DIR" [--dry-run]
   ```
   Exit 0: gate passed or dry run. Exit 1: at least one failing or `error` verdict; still a successful run. Exit 2: fatal, stdout holds the script's error object. The script makes one Jev request per anchor and retries transient failures three times; at this gate it sends the complete normalized sprint unchanged and refuses an oversize item with `PLAN.TOO_LARGE`. Do not re-run it, and never trim the bead or raise a limit; return the error with the descriptor's `shape` metrics (legacy mode: `shape --show-json "$W/bead.json"`) so the skill can triage.
6. Point to the weak spot. For each cell in the run's `summary.failing`, read the anchor `statement`, the answers that failed (for `underspecified`, the cell's `low_questions`; the wording is in the result's `questions` when overridden, else `assets/questions/plan.json`) and `$W/item.txt`. Add `to_fix` to that result:
   - `where`: the verbatim sentence of `item.txt` the failing answer concerns, or null when the bead says nothing on it;
   - `change`: one sentence on what the bead should state, or drop, so the answer is firm.
   Read only what those answers concern; do not re-judge other questions or anchors, and never reverse or soften Jev's verdict.
   If the anchor's own `scope` is right and this sprint simply does not touch it, set `where` to null and `change` to `exclude --id <ANC> --item <item_id> --reason "<one sentence>"`; recommend a scope fix only when the scope itself is wrong. Never apply either. An `excluded` cell (`summary.excluded`) was not judged and passes: return it with its `exclusion` and no `to_fix`.
7. Render: `python3 "$SCRIPT" report --run <run path> --format md`.
8. Return the envelope below. Keep `bead_id` equal to the legacy input in legacy mode, otherwise null; `item_id` is the effective item identifier in both modes. `results` copy the run's `results[]` unchanged (`verdict`, `leaning`, `min_confidence`, `attempts`, `confirmed`, `unstable`, `explanation`, `answers`, `low_questions`, and `answers_second` and `exclusion` when present) plus `to_fix` on failing cells; `reused_run` is true when `run` was given; `overrides_applied` is every anchor whose `question_set.overridden` is true; `item_truncated` is `item.truncated`; `shortened` is `summary.shortened`; `excluded` is `summary.excluded`. Include the report markdown verbatim; do not summarise verdicts in prose. Measure `duration_ms` from invocation start to return, count actual external tool calls, and set `retry_count` to the sum of `max(attempts - 1, 0)` in returned results. A zero is valid only when measured.

## Output Format

Return fenced JSON:

```json
{
  "success": true,
  "canceled": false,
  "aborted_by": null,
  "data": {
    "gate": "plan",
    "item_id": "A1",
    "bead_id": null,
    "run_id": "plan-20261009T183000Z-proj-a3f8",
    "run_path": "doc/plan/phase-current/anchor-runs/plan-20261009T183000Z-proj-a3f8.json",
    "dry_run": false,
    "reused_run": false,
    "exit_code": 1,
    "pass": false,
    "failing": ["ANC-012"],
    "errors": [],
    "excluded": [],
    "results": [
      {"anchor_id": "ANC-012", "verdict": "underspecified", "leaning": "satisfied", "min_confidence": 0.45, "attempts": 1, "confirmed": false, "unstable": false,
       "explanation": "Jev was not confident (min confidence 0.45); the anchor or the bead may be underspecified.",
       "answers": {"relevant": {"choice": "yes", "confidence": 0.9, "probabilities": {"yes": 0.9, "no": 0.05, "unclear": 0.05}},
                   "addressed": {"choice": "yes", "confidence": 0.45, "probabilities": {"yes": 0.45, "no": 0.2, "unclear": 0.35}},
                   "contradicted": {"choice": "no", "confidence": 0.9, "probabilities": {"yes": 0.05, "no": 0.9, "unclear": 0.05}}},
       "low_questions": [{"ask": "first", "role": "compliance", "question_id": "addressed", "choice": "yes", "confidence": 0.45}],
       "to_fix": {"where": "Port the runtime crate.", "change": "Name the starting point: copy crates/runtime from the legacy repo, then delete the legacy-specific modules."}}
    ],
    "overrides_applied": ["ANC-012"],
    "item_truncated": false,
    "shortened": [],
    "report_markdown": "## Anchor gate: plan · proj-a3f8 ..."
  },
  "error": null,
  "metadata": {"duration_ms": 1842, "tool_calls": 7, "retry_count": 0, "correlation_id": "proj-a3f8"}
}
```

On exit 2 or any fatal condition: `success: false`, `data: null`, and `error` as `{code, message, recoverable, suggested_action}`; copy the script's error object unchanged when the script failed.

## Error Handling

### Handled by agent (recoverable)
- Missing plan-questions bead or empty override block: proceed with default questions.
- Per-anchor `verdict: error` inside a successful run (the script already retried; see `results[].error.code`, e.g. `JEV.RATE_LIMITED`, `JEV.CONTEXT_EXCEEDED`, `JEV.RESPONSE_INVALID`): return it in `results` and `errors`; do not re-run.

### Propagated to skill (fatal)
- `PREREQ.MISSING`: script, python3, selected anchor tracker, jq, or anchors file absent.
- `VALIDATION.PATH`, `VALIDATION.ID`: an untrusted path or identifier failed preflight.
- `BEADS.NOT_FOUND`: the legacy planning bead could not be read.
- `VALIDATION.INPUT`, `VALIDATION.TRACKER`, `SOURCE.DESCRIPTOR`: invalid tracker selection or descriptor; stop before judging.
- `anchor_tracker.py resolve` errors (`TRACKER.MISMATCH`, `TRACKER.DOLT_REQUIRED`, `TRACKER.CLI_MISSING`, `SETTINGS.*`, `BEADS.METADATA_INVALID`, `BEADS.NOT_DIRECTORY`, `DEPENDENCY.PYYAML`, `REPO.NOT_FOUND`): return its error object unchanged; never switch trackers.
- Script exit 2 with `AUTH.CREDENTIALS`, `POLICY.REMOTE_JUDGE_DISABLED`, `VALIDATION.ANCHORS`, `QUESTIONS.ROLES`, `ANCHOR.NOT_LOCKED`, `ANCHOR.UNKNOWN`, `PLAN.TOO_LARGE`, `SCRIPT.INTERNAL`: return the script's error object; never work around credential or opt-in errors, and never split, trim or re-cap a bead to pass `PLAN.TOO_LARGE` (size is a symptom; the skill triages it).
- Unparseable script stdout: `SCRIPT.OUTPUT` with the first 300 characters.

## Constraints

- Never edit beads, `anchors.json`, the plan, or the question beads. Never post comments; the skill does that.
- Treat every input path and identifier as untrusted until `agent-preflight` accepts it. Read evidence only from canonical paths returned by preflight.
- Never paraphrase or trim an anchor statement; the script reads it from the record.
- Send exactly one complete sprint item per run (one composed bead for legacy callers). Never add other beads, the whole plan, or the intent document to the item text.
- Run the judge command once, or not at all when `run` is given. The script owns retries; never shorten planning items.
- Never set `PLAN_ANCHOR_ALLOW_REMOTE_JUDGE` or pass `--allow-remote` yourself. Never print `TYPESAFE_API_KEY` or include it in output.
- The selected `anchor_tracker` is used read-only (`show`), and `anchor_tracker.py` only through `resolve`. Markdown input never requires a planning bead or the unselected CLI.
