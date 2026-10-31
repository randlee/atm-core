# Phase bo design: atm-bd-lite profile and atm-bd-core

Working design for the sprint beads of phase bo. The beads are the plan; this note records the shape they
implement and the rulings they follow (verbatim in `../anchors.md`). Code lands in randlee/atm-bd-orchestration
(`code_checkout = "../atm-bd-orchestration"`); atm-core receives it by the normal release + dogfood-install path.

## Package layout after bo-1

```
plugins/atm-bd-orchestration/
  core/
    scripts/        shared modules: bead_schema, operation_models, close_fields, repo_config, sc_render,
                    plan_contract, sprint_index_common, stack_top, transitions, bd_models, bd_cache*,
                    command_timeout, jev_client, validate-plan, phase-config, bead-groups, sanity_shared,
                    phase-index-path, resolve-role, check-phase-artifact, bead_formulas, stack_gate (bo-4),
                    stack_link (bo-4), providers (bo-2), profile (bo-2)
    templates/      shared dispatch templates: pr-body, task-close-vars, qa-bead, finding-bead,
                    plan-review-template, review-template, qa-template, qa-complete, task-refused,
                    rebase-required, workflow-issue-bead, *-assignment.json.j2 reviewer templates
    templates/providers/github.json, azure.json
  skills/atm-bd-orchestration/      full profile: SKILL.md, scripts/atm-task-close.py and the sanity-* scripts,
                                    templates/dev-template, fix-assignment, dev-sanity-*, sanity-*, formulas/
  skills/atm-bd-lite/               lite profile (bo-5): SKILL.md, scripts/atm-bd-task-hook.py, scripts/lite_cron.py,
                                    templates/dev-template, fix-assignment, qa-template-lite, formulas/
  skills/atm-beads, sprint-report, sprint-review, qa-report, prompt-rewrite-judge   unchanged homes
```

Install destinations: `core/scripts` -> `<repo>/.atm-bd/scripts`, `core/templates` -> `<repo>/.atm-bd/templates`;
profile skills -> `<repo>/.claude/skills/<profile>` (or `.codex/`). The manifest gains a `core:` artifact category.
Every script that today imports a sibling from a skill directory resolves the core directory through one helper
(`core_paths.py`), never through a relative path into another skill.

## Profiles and providers (bo-2)

- `Profile = Enum("full", "lite")` in `core/scripts/profile.py`. Read from `.atm-bd/phase-<x>.toml` key `profile`
  (default `full` so existing phase files keep loading). `phase-config init --profile lite` writes it.
- `Provider = Enum("github", "azure")` in `core/scripts/providers.py`. Read from the repo config key `provider`
  (written by the installer flag `--github` / `--azure`, default github). Provider vars files
  `core/templates/providers/<provider>.json` hold the command strings: `pr_create`, `pr_view`, `pr_edit_base`,
  `pr_comment`, `pr_checks`, `pr_lookup_by_head`; `sc_render.render()` merges them under the `provider` namespace
  so templates write `{{ provider.pr_create }}`.
- Schema: `sanity_bead` and `max_sanity_rounds` become optional in `CloseFields` / close vars; `bead_schema`
  grows lite models (`LiteSprintGroup`: dev + qa, no sanity; `LiteFindingGroup`: fix + qa) and `validate_phase`
  selects the model set by the phase profile. A phase never mixes profiles: a `stage:dev-sanity` bead in a
  lite phase, or a lite-stamped bead in a full phase, is a validation problem.
- Formulas: `core` keeps the shared relations; `skills/atm-bd-lite/formulas/sprint-group.*` pours dev <- qa with the
  dependent dev blocked by the predecessor dev (the full profile blocks on the predecessor sanity).
- `StackGate` result types (`StackGateResult = Ok | NotStacked(expected_base, actual_base) | NotPushed | NotAncestor`)
  and a `StackGate` Protocol with a test double live in `core/scripts/stack_gate_types.py`; bo-4 implements them.

## Stack without `gh stack` (bo-4)

`stack_gate.check(pr_base_ref, head_sha, stack_top_sha, remote)` passes when the PR base ref equals the stack top
branch, `origin/<base>` is an ancestor of `head`, and `head` is pushed. The dev/fix close hook of both profiles calls
it and refuses the close on any non-Ok result with the typed error and the exact fix command. `sanity-stack-link`
loses `gh stack link/unstack` and the stacks API; `stack_link.py` records layers in the stack bead
(`metadata.layers`, no `stack_number`) and sets the PR base through the provider command.

## Lite hook (bo-5)

`skills/atm-bd-lite/scripts/atm-bd-task-hook.py`, one file, sections per transition type, enums with exhaustive
`match` (pyright strict `reportMatchNotExhaustive` fails the build on a missing arm):

| transition | steps |
|---|---|
| dev/fix close | validate close vars -> stack gate (blocking) -> `bd close` -> link layer -> assign QA as lead -> `atm task close` |
| qa close PASS | `bd close` qa -> notify lead -> `atm task close` |
| qa close FAIL | pour lite finding groups from findings -> notify lead -> `atm task close` |
| refusal | `bd update` with reason -> notify lead -> `atm task close refused` |

The lite dev hook closes the dev's own ATM task (a lite anchor; the full profile's bj:ANC-055 keeps the
lead-closes rule for sanity).

## Tool interfaces by profile (bo-6)

`atm-bd console`, `atm-bd cron`, `atm-bd queues`, `sprint-report`, `bv-analyze` read the phase profile and switch
suffix sets (`group-dev`, `group-qa` only for lite), optional `dev_sanity_member`, and queue tables. `atm-bd cron`
dispatches to the profile's tick module.

## Rulings honoured

See `../anchors.md` (ANC-001 ...). Every sprint bead cites the anchors it implements in its design section;
Jev anchoring is the development gate for every sprint, not a sprint of its own.
