# Phase bo: atm-bd-lite

Spec: `intent.md` and `hooks/task-{assign,start,close}.md` (Rand-owned, ANC-071). Anchors: `anchors.json` (rendered in `anchors.md`). Sprint docs are the plan; this file gives order and coverage only.

## Order

| # | Sprint | Repo | Consumes | Notes |
|---|---|---|---|---|
| 1 | bo-9 `sprint-bo-9-types-020.md` | atm-bd-orchestration | — | atm-bd-types 0.2.0; stub NOT APPROVED until Rand approves it |
| 2 | bo-12 `sprint-bo-12-jev-check-gate.md` | atm-bd-orchestration | — | lands before bo-10 (brief §B "Ordering") |
| 3 | bo-11 `sprint-bo-11-lite-templates.md` | atm-bd-orchestration | bo-9 metadata names | pour block blocked on beads-cache#104 |
| 4 | bo-10 `sprint-bo-10-lite-hooks.md` | atm-bd-orchestration | bo-9 stub, bo-12 gate, bo-11 phase-file template keys | |
| 5 | bo-15 `sprint-bo-15-collect-decide.md` | atm-bd-orchestration | bo-9 types and CacheStore | adds atm-bd-types 0.3.0 monitor module |
| — | bo-13 `sprint-bo-13-hook-wire-audit.md` | atm-core | — | parallel; independent of bo-9..15 |
| 6 | bo-14 `sprint-bo-14-lite-live.md` | atm-bd-orchestration | bo-9..12, bo-15, bo-13 audit log | live run needs a released `bdc` (sc-publish#135) and beads-cache#104 |

bo-9, bo-12 and bo-13 can be developed in parallel; each layer rebases onto the stack top in the order above.

## External blockers
- randlee/beads-cache#104 (create): the bo-11 pour block and the bo-14 QA-findings round.
- `bdc` binary release (blocked on randlee/sc-publish#135): the bo-14 live run.
- No OpenAI key: Luna is unverified; the bo-12 live evidence covers Jev and Haiku.

## Anchor coverage (plan gate, 2026-11-01)
Sprints whose plan-gate verdict for the anchor is satisfied (or deterministic-only for structural anchors); no sprint contradicts any anchor. Runs: `anchor-runs/plan-gate-2026-11-01*`.

| Anchor | Statement | Satisfied by |
|---|---|---|
| ANC-001 | The installer must accept --github and --azure flags that select the provider commands ... | bo-14 |
| ANC-003 | Transitions in the lite hook must be enum members with exhaustive checks, so that an un... | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-004 | Test code written in the phase must implement its repeated patterns through common help... | bo-10 |
| ANC-005 | atm-bd cron, atm-bd console and atm-bd queues must be invoked with the same subcommands... | bo-14, bo-15 |
| ANC-006 | All profiles must use the same schema. | bo-9 |
| ANC-012 | Lite-only scripts must live in skills/atm-bd-lite/scripts and lite-only templates in sk... | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-014 | The atm-bd-lite skill must have its own task-hook Python file, separate from the atm-bd... | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-015 | The lite profile's orchestration must be runnable by a single agent. | bo-11, bo-14 |
| ANC-016 | New or rewritten Python code in the phase must return Result<T,Err> values instead of t... | bo-10, bo-15 |
| ANC-017 | The fundamental atm-bd types must be defined in one Rust crate. | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-018 | A quick Python-only fix of atm-bd must release without a crate publish step. | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-019 | The lite task-hook file must be organized by transition type: one handler section per t... | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-024 | The console never dispatches work for a phase whose phase file sets the lite profile. | bo-15 |
| ANC-026 | The lite task hook and the lite dispatch templates must take the profile from the phase... | bo-10, bo-11 |
| ANC-027 | Each atm-bd cron tick must take the profile from the phase file and refuse to run when ... | bo-14, bo-15 |
| ANC-028 | atm-bd-lite must be its own skill in the skills/atm-bd-lite/ directory. | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-030 | atm-bd-orchestration skill files must not be changed to use the shared code. | bo-9, bo-10, bo-14, bo-15 |
| ANC-031 | Each profile's task hooks, task-assignment templates and cron script must be inside tha... | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-032 | Each atm-bd subcommand must run the script with the same file name from the skill of th... | open: see note |
| ANC-034 | Every atm-bd-types trait must ship a .pyi stub generated from the trait. | bo-9, bo-15 |
| ANC-036 | Code in the shared location must be newly written, not moved or copied out of atm-bd-or... | bo-9, bo-14, bo-15 |
| ANC-038 | Every Python implementation of an atm-bd-types trait must pass pyright strict against t... | bo-9, bo-10 |
| ANC-039 | No change made for atm-bd-lite may break atm-bd-orchestration. | bo-9, bo-10, bo-11, bo-12, bo-14, bo-15 |
| ANC-040 | atm-bd-lite code that is not specific to the lite profile must be placed in the shared ... | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-041 | Every public function and trait implementation in the shared location must have a test ... | bo-15 |
| ANC-042 | Every public function and trait implementation in the shared location must have a test ... | bo-15 |
| ANC-044 | Code in the shared location must not branch on, or hard-code, either profile. | bo-9, bo-14, bo-15 |
| ANC-045 | atm-bd-lite must call each shared-location module in the same sprint that adds the module. | bo-15 |
| ANC-048 | The lite task.assign hook must be its own Python file under skills/atm-bd-lite/scripts/. | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-049 | The lite task.start hook must be its own Python file under skills/atm-bd-lite/scripts/. | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-050 | The lite task.close hook must be its own Python file under skills/atm-bd-lite/scripts/. | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-051 | Each lite task hook file must have its own state-transition diagram in the phase design... | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-052 | A lite hook's state-transition diagram must have one Transitions-table row per match ar... | bo-9, bo-10, bo-11, bo-12, bo-13, bo-14, bo-15 |
| ANC-053 | Jev must judge every Transitions-table row of a lite hook's diagram as implemented by t... | bo-10, bo-12 |
| ANC-054 | Jev must judge every match arm in a lite hook's code as listed in that hook's state-tra... | bo-10, bo-12 |
| ANC-055 | The lite profile must not write or read a close-vars file or any other per-task state f... | bo-10, bo-11, bo-12 |
| ANC-056 | The lite hooks must look up every fact other than the task id at hook time from bd, git... | bo-10 |
| ANC-057 | A lite dev or fix closes its task with one atm task close command carrying its report a... | bo-10, bo-11 |
| ANC-058 | When a lite hook cannot look up a fact, it must refuse with the exact command that fixe... | bo-10 |
| ANC-059 | The lite profile must have no repair, backfill or legacy-compatibility script. | bo-10, bo-12, bo-14 |
| ANC-060 | atm-bd-lite scripts, hooks and templates must not invoke gh pr list, gh pr checks, gh r... | bo-10, bo-11, bo-12, bo-15 |
| ANC-061 | atm-bd-types must not define profile names: no profile enum and no profile string liter... | bo-9, bo-15 |
| ANC-062 | atm-bd-lite code must not pass or hold hook input, bead data, phase-file data or replie... | bo-9, bo-10, bo-12, bo-14, bo-15 |
| ANC-063 | Every atm-bd-lite hook, script and agent step must read and write beads only through be... | bo-9, bo-10, bo-11, bo-12, bo-15 |
| ANC-064 | A lite hook must make at most one beads-cache write and must never create, claim or rel... | bo-9, bo-10, bo-12 |
| ANC-065 | A lite hook must never write the stack bead (the ledger). | bo-10, bo-12 |
| ANC-066 | A lite hook must not run a subprocess or use the BdCli, GitCli or AtmCli adapters, bd o... | bo-9, bo-10, bo-12 |
| ANC-067 | Every multi-step procedure a lite agent must run (stack, pour) must be a fenced bash bl... | bo-11 |
| ANC-068 | The generated atm_bd_types stub must carry only names that lite or atm-bd-core code use... | bo-9, bo-10, bo-15 |
| ANC-069 | The stack bead must be the ledger: its assignee is the lock, its layers are the record,... | bo-11 |
| ANC-070 | Each lite hook file must be at most 100 lines, including header and docstring. | bo-10, bo-12 |
| ANC-071 | docs/plans/phase-bo/intent.md and docs/plans/phase-bo/hooks/*.md must change only in co... | bo-9, bo-10, bo-12 |
| ANC-072 | A review finding that would add a write, an arm, a subprocess or a guard to a lite hook... | bo-10, bo-12 |
| ANC-073 | Every phase-bo anchor check must name a command (grep, wc, ast, pyright, the gate) or a... | open: see note |
| ANC-074 | The lite cron script must assign each QA task that bdc ready reports as ready and unass... | bo-14, bo-15 |
| ANC-075 | The atm-bd queues, cron, console (TUI), show and snapshot subcommands must work for bot... | bo-15 |
| ANC-076 | The atm-bd monitoring code for lite must be two separate layers in atm-bd-core: collect... | bo-9, bo-10, bo-14, bo-15 |

Open: ANC-032 (rewording proposed to Rand, its wording predates the PhaseMonitor design); ANC-073 is checked on `anchors.json` itself and passes.
