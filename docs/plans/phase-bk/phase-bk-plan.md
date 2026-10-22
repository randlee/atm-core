# Phase bk — atm task hooks, hook registration, atm cron, doctor hook section

Authority: `design.md` (fenix, PR #1877) over Rand's verbatim rulings on
atm-core #1873 (2026-10-18). Anchors: `anchors.md` (ANC-001..025,
REQ-BK-*). Intent: `intent.md`. Lead: fenix. Integration branch
`integrate/phase-bk`, cut from `develop`; every sprint PR targets it or the
layer below on the one phase stack. Requirements: `docs/requirements.md`
§11.5, §23, §24 (`REQ-P-HOOK-001..004`, `REQ-CORE-HOOK-001..002`,
`REQ-P-CRON-001..002`). ADRs: ADR-068 (hook contract and runner), ADR-069
(`atm cron`), ADR-061 D6 record of 2026-10-18; all three landed with this
plan PR, as ADR-066 and ADR-067 did with the phase-bi plan.

## Sprints

The beads `atm-bk-1` .. `atm-bk-8` are the plan; each bead's vars are
generated verbatim from its sprint doc, which carries the contract
signatures and design table rows it owns. Layers are unique and each PR
targets the layer below.

| Layer | Sprint | Crates | Tracker | Branch → PR target | Closure |
| --- | --- | --- | --- | --- | --- |
| 1 | bk-1 store contracts, codes, budgets | `atm-storage`, `atm-error` | `atm-bk-1` | `sprint/bk-1-hook-stores` → `integrate/phase-bk` | `contract` |
| 2 | bk-2 core hook types and sealed traits | `atm-core`, `atm-runtime-test-support` | `atm-bk-2` | `sprint/bk-2-hook-contract` → layer 1 | `contract` |
| 3 | bk-6 SQLite hook and cron stores | `atm-storage-rusqlite` | `atm-bk-6` | `sprint/bk-6-hook-stores-sqlite` → layer 2 | `boundary` |
| 4 | bk-3 daemon hook runner | `atm-daemon-bootstrap` | `atm-bk-3` | `sprint/bk-3-hook-runner` → layer 3 | `boundary` |
| 5 | bk-7 filter, dispatcher, routes, HTTP 1.13.0 | `atm-http-runtime`, `atm-core` protocol | `atm-bk-7` | `sprint/bk-7-hook-filter` → layer 4 | `boundary` |
| 6 | bk-4 `atm-cron` | `atm-cron` (new) | `atm-bk-4` | `sprint/bk-4-atm-cron` → layer 5 | `boundary` |
| 7 | bk-5 CLI, composition, docs | `atm`, `atm-runtime`, `atm-daemon-bootstrap` composition | `atm-bk-5` | `sprint/bk-5-hook-cli-doctor` → layer 6 | `integration` |
| 8 | bk-8 per-member `max_tasks`, started-first order, one nudge per member (#1932) | `atm-storage-rusqlite`, `atm-storage`, `atm-core`, `atm`, `atm-http-runtime` | `atm-bk-8` | `sprint/bk-8-max-tasks` → `integrate/phase-bk` (parallel_safe; rebased onto the top when done) | `integration` |

## Wave table

| Wave | Sprints | Hard dependency (consumed artefact) |
| --- | --- | --- |
| 1 | bk-1, bk-2 | bk-2 ← bk-1: the store traits and types, doubles, codes, constructors, budget constants as written in `sprint-bk-1-hook-stores.md` (Public contract); bk-2's dev codes against that text from the start and closes only after bk-1 completes |
| 2 | bk-6, bk-3, bk-7 | bk-6 ← bk-1: the two store traits and their row types, `CronSchedule.generation`, the contract tests' expectations. bk-3 ← bk-2: `HookRunner`, `HookTarget`, `HookRequest`, the reply parsers, doctor request/reply. bk-7 ← bk-2: `TaskCommands`, `TaskCommand::classify`, `HookActionDispatcher`, `DispatchSpec`, the request structs, `check_limits`, the doubles |
| 3 | bk-4 | bk-4 ← bk-2: `CronScheduler`, `CronStartSpec`, `HookRequest::for_cron`, `DispatchSpec`, the doubles; ← bk-3: its root `Cargo.toml`, `Cargo.lock` and `.just/lint-config.toml` edits (Rand, 2026-10-19: "do 4 waves") |
| 4 | bk-5 | bk-5 ← bk-6: `StorageHandles` accessors; ← bk-3: `DaemonHookRunner`, `fixture_hook.py`; ← bk-7: routes, `HookedTaskCommands`, `ChainTracker`, `install_cron_scheduler`; ← bk-4: `AtmCron`, `CronWorkerHandle`, `ManualClock` |
| 1 | bk-8 | none: parallel_safe; consumes nothing from bk-1..bk-7 and shares no owned path with them (GitHub #1932) |

Critical path 4 waves (Rand, 2026-10-19: "do 4 waves"); bk-4 must_follow bk-3, a ruling that overrides `atm-beads-plan-guidelines.md:177`. bk-1 and bk-2 are separate sprints developed
concurrently by two agents (Rand, 2026-10-19: "I would do them in parallel w/ 2 agents."; and: "that works, he starts, then when bk-1 completes, he finishes"). bk-2's dev
starts with bk-1's, coding `atm-core` against bk-1's atm-storage contract
types in `sprint-bk-1-hook-stores.md`. bk-2 closes only after bk-1
completes: it rebases onto bk-1's final head, runs its gates on the real
`atm-storage` code, then closes; it never closes ahead of bk-1. Stack order
stays bk-1 then bk-2, bk-2's PR targeting bk-1's branch. In beads,
`atm-bk-2.group-sanity` is blocked by `atm-bk-1.group-dev` and
`atm-bk-2.group-dev` is ready in wave 1. The CLI and composition (bk-5)
need every runtime crate. Other parallel sprints are cut from their
dependency's head and rebased into the stack in layer order when done
(Rand 2026-10-12). bk-4 starts from bk-3's final head and builds on its
root `Cargo.toml`, `Cargo.lock` and `.just/lint-config.toml` edits.

## Decisions

Decisions are in `design.md` (§2.2, §5, §6, §9); Rand's verbatim rulings
are on #1873 and PR #1877. ADR-069 D5 amends ADR-063 D3 to fourteen
semantic storage capabilities (Rand: "yes, we need storage for cron").

## Governed interfaces (ADR-061)

- SQLite storage: two additive tables, MINOR; D6 record of 2026-10-18 on
  `docs/adr/ADR-061-governed-interface-schema-versioning.md` (this PR);
  bk-6 lands the D3 previous-consumer proof test.
- HTTP API: six additive `RequestEnvelope` variants, the `HookActions`
  response and the `X-ATM-Hook-Actions` header; `HTTP_API_VERSION`
  `1.12.0` → `1.13.0` in bk-7, with the D5 record line, both
  `openapi.yaml` copies and the six paths.
- Hook stdin/stdout contract: `HOOK_SCHEMA_VERSION` independent of
  `HTTP_API_VERSION` (ADR-068 D8); one key-pinning test in bk-2.
- HTTP previous-consumer proof: bk-7's pinned 1.12 fixture test.
- Herdr IPC: unchanged.
