---
name: bead-layout
version: 0.6.0
description: The anchor bead tree in beads (bd), the commands to create it, labels, how question overrides live in the child beads, how agents read beads, and how reports are posted back.
---

# Bead layout

Jev questions live in beads so they version with the work and can be edited per gate without touching the anchor record. One tree per fact:

```
ANC-012 anchor bead            (type decision, labels: anchor, anchor:ANC-012)
├── plan-questions bead        (type decision, labels: anchor, anchor-questions, anchor:ANC-012, gate:plan)
└── dev-questions bead         (type decision, labels: anchor, anchor-questions, anchor:ANC-012, gate:dev)
```

REQ and ADR anchors get the same tree under their own ID. Bead IDs carry the repository prefix chosen when the tracker workspace was created (`proj-a3f8`, children `proj-a3f8.1`); `bd-42` in these documents is illustrative.

## Creating the tree

With `$S` and `$A` resolved as in `SKILL.md`:

```bash
ID=ANC-012
T="$(dirname "$S")/../assets/templates/anchor-questions-bead.md"
STATEMENT=$(python3 -c "import json;print(next(a['statement'] for a in json.load(open('$A'))['anchors'] if a['id']=='$ID'))")
QUOTE=$(python3 -c "import json;a=next(a for a in json.load(open('$A'))['anchors'] if a['id']=='$ID');print(a.get('quote') or a['statement'])")
SUMMARY=$(python3 -c "import json;print(next(a['summary'] for a in json.load(open('$A'))['anchors'] if a['id']=='$ID'))")

ANCHOR=$(bd create "$ID: $SUMMARY" --type decision --labels "anchor,anchor:$ID" --silent \
  --description "$STATEMENT" \
  --notes "Quote (verbatim, immutable): $QUOTE
Anchor record: $A. The description is the testable statement; change it only through jev_judge.py update --statement.")
PLANQ=$(bd create "$ID plan questions" --type decision --parent "$ANCHOR" --labels "anchor,anchor-questions,anchor:$ID,gate:plan" --silent --body-file "$T")
DEVQ=$(bd create "$ID dev questions"  --type decision --parent "$ANCHOR" --labels "anchor,anchor-questions,anchor:$ID,gate:dev"  --silent --body-file "$T")
python3 "$S" set-beads --anchors "$A" --id "$ID" --anchor-bead "$ANCHOR" --plan-bead "$PLANQ" --dev-bead "$DEVQ"
python3 "$S" render --anchors "$A"
```

Labels are passed explicitly on the children so the tree is correct on a `bd` that does not inherit parent labels. Anchor beads are type `decision` and labelled `anchor`; keep them out of work queues with `bd ready --exclude-label anchor`.

## Question overrides in the child beads

The wording of record is `assets/questions/plan.json` and `assets/questions/dev.json`. A question bead's description holds one fenced JSON block; `{}` means defaults. To override for this anchor only, give just the questions to replace, keyed by the default IDs, keeping the `yes`/`no`/`unclear` options:

````markdown
```json
{
  "questions": {
    "addressed": {
      "type": "choice",
      "instructions": "Does `item.text` state that the runtime crates are copied from the legacy repo rather than written new? ...",
      "criteria": {"yes": "...", "no": "...", "unclear": "..."}
    }
  }
}
```
````

Edit with `bd update <question bead> --body-file <file>`. Overrides may also live in `anchors.json` under `question_overrides.plan` or `.dev`; the bead wins when both exist. Prefer the bead: each gate evolves its wording independently against one root fact. Never rename question IDs; the script refuses a set whose roles no longer resolve (`QUESTIONS.ROLES`).

## How the agents read beads

- `bd show <id> --json` prints a **one-element JSON array**. Text fields: `title`, `description`, `design`, `acceptance_criteria`, `notes`; an empty field is **absent**. Agents do not parse it: `python3 "$S" compose-item --show-json <file or -> --out <item.txt>` composes `# Title`, `## Description`, `## Design`, `## Acceptance criteria`, `## Notes` from what is present.
- `bd children <id> --json` lists children with `status`, `title`, `labels`, and for closed children `close_reason` and `closed_at`. `compose-item --children-json <file>` appends `## Closed beads`, one line per closed child.
- Question beads resolve from `anchors.json` (`beads.plan_questions` / `beads.dev_questions`) first, then `bd children <anchor bead> --json` filtered on the `gate:plan` or `gate:dev` label.
- The fenced JSON from the question bead's `description` goes into an overrides file keyed by anchor ID, passed as `--overrides-file`; the script merges.

All read-only. Agents never create, update, close or comment on beads.

## Posting results back

The planning gate posts its report on the judged planning bead; the development gate on the sprint bead or the PR. Done in the main session after the agent returns:

```bash
python3 "$S" report --run doc/plan/phase-current/anchor-runs/<run>.json --format md > "$W/report.md"
bd comment <bead> --file "$W/report.md"          # or: bd comment <bead> "$(cat "$W/report.md")"
```

`bd comment` takes `--file` or `--stdin`; `--body-file` belongs to `bd create` and `bd update` only.
