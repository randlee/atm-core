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
                    stack_link (bo-4), providers, profile, pr_lookup, result, stack_gate_types (bo-2),
                    hook_runner (bo-5)
  crates/atm-bd-types/   Rust crate: fundamental types, PyO3 wheel (bo-2)
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

## Fundamental types: the `atm-bd-types` crate (bo-2)

Rand (2026-10-31): a Rust crate may own the fundamental types if it is easy to distribute, and the crate must not
need publishing for quick Python fixes. So `crates/atm-bd-types` (PyO3 behind a feature, abi3-py311, stubs by
`pyo3-stub-gen`) is published to crates.io and as a PyPI wheel by its own manual `crate-release.yml`, at its own
version; the Python package pins a compatible range and `release.yml` never touches the crate. The crate defines:
`Profile {Full, Lite}`, `Provider {Github, Azure}`, `Transition {DevClose, FixClose, QaPass, QaFail, Refused}`,
`ErrorKind` + `Error {kind, message, fix}`, `StackGateResult`, `PrOp`, `ProviderOp`, `ProviderCommands`, `CloseVars`.
Python redeclares none of them.

## Profiles and providers (bo-2)

- `Profile` is read from `.atm-bd/phase-<x>.toml` key `profile` (default full so existing phase files keep loading)
  by `core/scripts/profile.py`; `phase-config init --profile lite` writes it. `require_profile(phase_file, expected)`
  is the one gate every hook, template render, cron tick and console run passes first; it returns the typed
  `ProfileMismatch` error (with the fix) when the phase file's profile is not the caller's (Rand, 2026-10-31:
  "the lite/orchestration flag is set in phase toml/json file ... hooks/templates/cron/console must all align").
- `Provider` is read from the repo config key `provider` (written by the installer flag `--github` / `--azure`,
  default github) by `core/scripts/providers.py`. The provider-operation boundary: `ProviderCommands` maps each
  `PrOp {Create, View, EditBase, Comment, Checks, LookupByHead}` to a `ProviderOp {argv with placeholders {base}
  {head} {title} {body_file} {number} {text}, body: None | File, output: Json | Text | ExitCode, result: number/url/base
  field map}`, loaded with serde from `core/templates/providers/<provider>.json`. An op runs as argv without a shell,
  a body travels only by `{body_file}`, output is parsed per `output` and the `result` map yields `PrRef {number, url,
  base}`. `sc_render.render()` exposes `provider.<op>` as the joined argv with placeholders for the template to fill
  (`{{ provider.create | replace("{base}", pr_target) }}`); `pr_lookup.py` executes `LookupByHead` for the scripts.
- Schema: `sanity_bead` and `max_sanity_rounds` become optional in `CloseFields` / close vars; `bead_schema`
  grows lite models (`LiteSprintGroup`: dev + qa, no sanity; `LiteFindingGroup`: fix + qa) and `validate_phase`
  selects the model set by the phase profile. A phase never mixes profiles: a `stage:dev-sanity` bead in a
  lite phase, or a lite-stamped bead in a full phase, is a validation problem.
- Formulas: `core` keeps the shared relations; `skills/atm-bd-lite/formulas/sprint-group.*` pours dev <- qa with the
  dependent dev blocked by the predecessor dev (the full profile blocks on the predecessor sanity).
- `StackGateResult {Stacked, NotStacked{expected_base, actual_base}, NotPushed{branch, head}, NotAncestor{base_sha,
  head_sha}}` comes from the crate; the `StackGate` Protocol with `check(pr_base_ref, head_branch, head_sha,
  stack_top_branch, remote="origin") -> StackGateResult` and the `FakeStackGate` double live in
  `core/scripts/stack_gate_types.py`; bo-4 implements `GitStackGate`.
- Errors are values: `core/scripts/result.py` (`Ok[T] | Err`, `Err` holding `atm_bd_types.Error` with a `fix`) is the
  single Result type; new code never raises for an expected failure, and `main` is the only place an `Err` becomes an
  exit code (Rand: "use pyright, return Result<T,Err> (no throwing)").

## Git-only stack gate (bo-4)

`GitStackGate.check(pr_base_ref, head_branch, head_sha, stack_top_branch, remote)` returns `Stacked` when the PR base
ref equals the stack top branch, `origin/<base>` is an ancestor of `head`, and `head` is pushed; one test maps an
Azure-style remote URL to a local bare repository with `insteadOf` to prove the gate is provider-agnostic. The dev/fix close hook of both profiles calls
it and refuses the close on any non-Ok result with the typed error and the exact fix command. `sanity-stack-link`
loses its hard-coded `gh stack link/unstack` and stacks API calls; `stack_link.py` records layers in the stack bead
(`metadata.layers`, no `stack_number`), runs the provider `StackLink` op and sets the PR base through the provider
command. On GitHub the ops stay `gh stack link` and `gh stack view --json`, so agent prompts keep the native stack
view (Rand, 2026-10-31: "we can and should use 'gh stack' in agent prompts if github is used. 'gh stack view --json'
is best view of stack and inexpensive if tokens available"); Azure gets the stack-bead equivalents.

## Lite hook (bo-5)

`skills/atm-bd-lite/scripts/atm-bd-task-hook.py`, one file, pure (imports only `enum`, `dataclasses`, `typing`,
`atm_bd_types`; no I/O): `plan(transition, vars, observed) -> Result[list[Command]]` matches `Transition`
exhaustively, one section per transition, and returns the command plan as data so a Rust cron can fire it under
monty or CPython (Rand: "cron for firing monty (or python hooks)"). `core/scripts/hook_runner.py` gathers the
observed state, calls `plan`, and executes the commands in order, skipping any whose `idempotent_when` already
holds and stopping at the first `Err` with the step, argv, stderr and the fix "rerun the same hook command";
a rerun re-derives the plan from observed state. No progress file. Steps per transition:

| transition | steps |
|---|---|
| dev/fix close | validate close vars -> stack gate (blocking) -> `bd close` -> link layer -> assign QA as lead -> `atm task close` |
| qa close PASS | `bd close` qa -> notify lead -> `atm task close` |
| qa close FAIL | pour lite finding groups from findings -> notify lead -> `atm task close` |
| refusal | `bd update` with reason -> notify lead -> `atm task close refused` |

The lite dev hook closes the dev's own ATM task (a lite anchor; the full profile's bj:ANC-055 keeps the
lead-closes rule for sanity).

## Tool interfaces by profile (bo-6)

`atm-bd console`, `atm-bd cron`, `atm-bd queues`, `sprint-report`, `sprint-review`, `bv-analyze` read the phase profile
with no new command or flag. The console never runs a lite phase: in lite the agents orchestrate, so
`atm-bd console` on a lite phase file exits with the typed `ProfileMismatch` error and dispatches nothing (Rand,
2026-10-31: "You can't run console for lite if agents are orchestrating. This is critically important"). cron, queues
and the reports switch suffix sets (`group-dev`, `group-qa` only for lite), optional `dev_sanity_member`, and queue tables. `atm-bd cron` dispatches to the profile's tick; the tick contract (argv, empty stdout or one
JSON object, exit code) is documented so the arriving Rust cron (sc-cron, once on crates.io) can fire it directly.
`bd_cache*.py` move unchanged and are marked as replaced by the arriving Rust library.

## Rulings honoured

See `../anchors.md` (ANC-001 ...). Every sprint bead cites the anchors it implements in its design section;
Jev anchoring is the development gate for every sprint, not a sprint of its own.
