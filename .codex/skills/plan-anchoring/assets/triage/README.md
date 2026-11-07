# Triage defaults

`judge` asks the questions in `plan-questions.jsonl` for every planning-gate cell that fails (turn this off with `--no-triage-questions`). They are asked in a second request, after the gate verdict is decided, so the gate request and every verdict are the same with or without them; the answers are recorded on the failed cell and never gate. A cell that passes is not asked. `rules.json` reads those answers and routes each failing cell to one of four routes; rules are tried in order and the first that matches wins.

| Route | Rule | Action |
| --- | --- | --- |
| `exclude` | `owns` no at 0.6 or more, and `contradicted` not yes | Record an exclusion (`exclude --from`) |
| `plan` | `owns` yes, `specific` no at 0.6 or more | The plan owner states the requirement in the item; `partial` yes means part is there |
| `question` | `owns` yes, `stated` yes at 0.6 or more, `specific` not no | Tune the gate question; the item text is a candidate pass fixture |
| `human` | anything else | Read the answers |

| Question | Asks |
| --- | --- |
| `owns` | Does this item itself deliver the requirement, or only depend on, re-run or defer it? |
| `specific` | Could a developer reading only the item deliver exactly what the requirement says? |
| `stated` | Is the requirement stated in its own sentence or clause? |
| `paraphrase` | Does the item say the same thing in other words? Recorded for review; no default rule uses it |
| `partial` | Does the item state some parts of the requirement and omit others? |

Measured on 41 planning-gate cells from one plan, each with an expected route frozen before the runs: `owns` answered no at 0.94 or more on all 8 cells owned by another item and yes on 11 of 12 owning cells, with no answer flips across three repeat runs; `specific` answered no at 0.61 or more on 8 of 9 cells where the requirement was only implied, and never no on the 12 cells that state it; `stated` answered yes at 0.66 or more on all 12 stating cells. One plan is narrow evidence: measure on your own plans and adjust the thresholds here.

## Add questions or change the defaults

The folder `.sc/plan-anchor/triage/` is yours; `judge` reads `plan-questions.jsonl` from the nearest one, else the shipped default. An upgrade never overwrites it.

- **Change a default:** edit that question's line in `plan-questions.jsonl`. Keep its `id`, because `rules.json` refers to the answers by id. Reword `instructions` or `criteria`; do not change `type`.
- **Add a question:** append one line, `{"id": "...", "type": "choice", "instructions": "...", "criteria": {"yes": "...", "no": "...", "unclear": "..."}}`. It is asked, with the others, for every failed cell and never gates. To use it in routing, add a condition on its `id` to a rule in `rules.json`.
- **Remove a question:** delete its line, and any `rules.json` condition that names it.
- **Try one without keeping it:** `judge --extra-questions my-questions.jsonl` adds it for that run only; this can be repeated.
- **Turn the defaults off:** `judge --no-triage-questions`. Extra questions you pass with `--extra-questions` still run.

Changing a question's text changes its answers: re-measure on your own plans before relying on a rule that uses it.

This folder belongs to the repository: the installer writes it once, when `.sc/plan-anchor/triage/` does not exist, and never overwrites it.
