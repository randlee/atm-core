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

## Boundary map

| Boundary | Crate | Sprint |
| --- | --- | --- |
| `BOUNDARY-HookRegistrationStore`, `BOUNDARY-CronScheduleStore` (new store traits, sealed), `HookName`, `PhaseId`, `HookRegistration`, `CronSchedule`, `NewCronSchedule`, `StopCondition`, in-memory doubles in `atm_storage::testing`; `hook_registration` and `cron_schedule` tables, rusqlite store impls (ADR-061 storage MINOR); the six `ATM_HOOK_*`/`ATM_CRON_*` error codes, constructors and catalog rows; the budget constants | `atm-storage`, `atm-storage-rusqlite`, `atm-error`, `boundaries/atm-storage*`, `docs/atm-storage`, `docs/atm-error-codes.md` | bk-1 |
| `HookRequest`/`HookReply`/`StopReply`/`HookAction` types; the four sealed core traits `TaskCommands`, `HookRunner`, `CronScheduler`, `HookActionDispatcher` with `TaskCommand::classify` and the doubles (no `Noop` types); protocol variants and routes (HTTP API 1.13.0); store accessors; doctor report types; double re-exports in `atm-runtime-test-support` | `atm-core`, `atm-runtime`, `atm-runtime-test-support`, `openapi.yaml` | bk-2 |
| `DaemonHookRunner` over one `spawn_with_deadline` helper; `NormalTaskCommands` (today's task path, extracted) and the filter `HookedTaskCommands` with `ActiveHookSnapshot`; `UnhookedRouter`, the only `HookActionDispatcher` impl; the six registry and schedule routes; composition of the filter; the spawn-site architecture test | `atm-daemon-bootstrap`, `atm-http-runtime`, `atm-architecture` | bk-3 |
| `atm-cron` crate: `AtmCron` impl of `CronScheduler`, worker loop (UTC next, stop check first, no missed-tick replay, one run in flight, reload on signal), `CronWorkerHandle`; `boundaries/atm-cron/cron-scheduler.toml`, forbidden edges, release entries | `atm-cron` (new), `boundaries/`, `.just/lint-config.toml`, `atm-architecture`, `release/` | bk-4 |
| `atm hook`, `atm cron` commands; exit-code arms; `atm doctor` hooks collection and rendering; bootstrap composition of `AtmCron`; user docs | `atm`, router `doctor()`, `atm-daemon-bootstrap` composition, `docs/user-documents` | bk-5 |

## Sprints

| Sprint | Authoritative plan | Tracker | Relation | Closure |
| --- | --- | --- | --- | --- |
| bk-1 | [hook registration and cron schedule stores](sprint-bk-1-hook-stores.md) | `atm-bk-1` | `root` | `contract` |
| bk-2 | [core hook contract, sealed traits and protocol](sprint-bk-2-hook-contract.md) | `atm-bk-2` | `must_follow` | `contract` |
| bk-3 | [hook runner and the task-command filter](sprint-bk-3-hook-runner.md) | `atm-bk-3` | `must_follow` | `boundary` |
| bk-4 | [atm-cron crate](sprint-bk-4-atm-cron.md) | `atm-bk-4` | `must_follow` | `boundary` |
| bk-5 | [hook and cron CLI, doctor hooks section, composition, user docs](sprint-bk-5-hook-cli-doctor.md) | `atm-bk-5` | `must_follow` | `integration` |

## Wave table

| Wave | Sprints | Hard dependency (consumed artefact) |
| --- | --- | --- |
| 1 | bk-1 | - |
| 2 | bk-2 | bk-1: `HookName`, `PhaseId`, `HookRegistration`, `HookRegistrationStore`, `CronSchedule`, `NewCronSchedule`, `StopCondition`, `CronScheduleId`, `CronScheduleStore`, the two in-memory store doubles, the six error codes and constructors (the sealed core traits take these types; `atm-core` depends on `atm-storage`, never the reverse) |
| 3 | bk-3, bk-4 (parallel, owned paths disjoint except hunks in root `Cargo.toml` and `.just/lint-config.toml`) | bk-3 ← bk-2: `TaskCommands`, `TaskCallCtx`, `TaskCommand::classify`, `HookRequest::for_task`, `HookReply::parse`, `HookAction::into_api_request`, `HookRunner`, `HookActionDispatcher`, the six request variants and routes, `HookActionsResponse`; bk-3 ← bk-1: `HookRegistrationStore::active_all`, `HOOK_ACTION_RESERVE_MS`. bk-4 ← bk-2: `CronScheduler`, `CronStartSpec`, `HookTarget`, `HookActionDispatcher`, `HookRunner`, `HookRequest::for_cron`, `StopReply`, the doubles; bk-4 ← bk-1: `CronScheduleStore`, `StopCondition`, `record_stop`, `record_run` |
| 4 | bk-5 | bk-5 ← bk-3: the six routes, the router's `hook_runner`, `install_cron_scheduler`, the four-tuple of `build_replacement_handler`, `fixture_hook.py`; bk-5 ← bk-4: `AtmCron`, `AtmCronDeps`, `CronWorkerHandle`, `SystemClock`, `TokioClock`; bk-5 ← bk-2: the envelope variants and doctor types |

Critical path 4 (bk-1, bk-2, bk-3 or bk-4, bk-5); width 2 (bk-3 with
bk-4); sprint count 5. Stack order after wave 3: both bk-3 and bk-4 carry
`pr_target = sprint/bk-2-hook-contract`; bk-4 is rebased onto bk-3's layer
when both are done (parallel-then-rebase, Rand 2026-10-12) and the lead
relinks it; bk-5 is cut from bk-4's layer. Earlier cuts: the first had the
CLI in wave 2 and a docs-only docs sprint; the second had the cron worker
in `atm-daemon-bootstrap` after the runner sprint; the third had one
contract sprint covering `atm-storage` and `atm-core`. Rand's ruling (PR
#1877, 2026-10-18: "I would expect atm cron to be in it's own crate w/
simple trait exposing it's features.") and ruthless-boundary-qa F001 moved
the worker into `atm-cron` behind the sealed core traits; the one contract
sprint grew past the plan gate's 24 000-byte reliable-review limit once the
three traits, the doubles, the stop condition and the protocol detail were
written at code level, so it is cut by crate into bk-1 (`atm-storage`,
`atm-storage-rusqlite`, `atm-error`) and bk-2 (`atm-core`), sequential
because the core traits take the storage row types. bk-5 composes the
crate, so it follows both runtime sprints.

## Decisions carried from design.md

D-1 fixed hook environment (`ATM_TEAM`, `ATM_PHASE`, `PATH`, `HOME`, plus
the Windows pair); D-2 no sha in atm, drift is `doctor()`'s; D-3 one
daemon-side runner, the daemon register route the only hook-name
validator; D-4 `atm-cron` crate behind `CronScheduler` (start, end, list),
`cron` crate 0.17 UTC, no missed-tick replay, one in-flight run per
schedule, args in the stdin request only; D-5 phases are independent
(Rand 2026-10-18): active phases' hooks run oldest first and the
chain stops at the first reply that is not not-applicable, any roster member may register or
start; D-6 `request` is
the bare HTTP body, an action is `{command, as, request}` over any routed
command except the hook and cron registry; D-7 contract types and traits
in bk-2, codes in bk-1; D-8 in-memory active-hook snapshot, `Local`
ingress, caller from the request; D-9 mandatory stop condition (`--until`
or `--until-hook`, `cron.stop`), `atm cron end` the manual stop; D-10
hooks are an input filter on the `TaskCommands` trait (Rand, 2026-10-18),
with no nesting variable or header, and a method with no registration
passes through. Replies are four results, `handled`, `pass-through`,
`not-applicable` and `refused` (Rand 2026-10-18, `design.md` §2.2). Review round 1 (2026-10-18) also carries D-a..D-r
(`design.md` §2.2, §5, §6, §9) and Rand's malformed and panic rulings;
Rand's 2026-10-18 rulings set D-l (registration-time `--custom` JSON),
D-q (cron timeout `CRON_TIMEOUT_MS`, at most 30 s) and D-r (per-schedule
jitter).

## Boundary review findings applied (ruthless-boundary-qa round 1, 2026-10-18)

RBQA-F001, F003, F004, F005, F006 (option B), F007 (action validity; the
authority half closed by Rand's round 4), F008, F009, F010, F011 and
the Q6 note (cron ingress `Local`) are applied in the sprint docs, ADR-068,
ADR-069, `design.md` and `docs/requirements.md`. F002 was ruled in round 5 (below).
F012 is not applied: `hook-runner.toml` stays `trait_only`.

## RBQA-F002 ruled: two new semantic storage capabilities

`HookRegistrationStore` and `CronScheduleStore` are the thirteenth and
fourteenth semantic storage capabilities; ADR-063 D3 capped the set at
twelve. Rand, round 5 (#1873 comment 6257361348, 2026-10-18, verbatim):
"yes, we need storage for cron". ADR-069 D5 is the one place that amends
ADR-063 D3 to fourteen. bk-1 updates the ADR-036 inventory and
`docs/atm-storage/boundaries.md` and owns those paths. No NEEDS RULING
item remains in this plan.

## Governed interfaces (ADR-061)

- SQLite storage: two additive tables, MINOR; D6 record of 2026-10-18 on
  `docs/adr/ADR-061-governed-interface-schema-versioning.md` (this PR);
  bk-1 lands the D3 previous-consumer proof test.
- HTTP API: six additive `RequestEnvelope` variants, the `HookActions`
  response and the `X-ATM-Hook-Actions` header; `HTTP_API_VERSION`
  `1.12.0` → `1.13.0` in bk-2, with the D5 record line, both
  `openapi.yaml` copies and the six paths.
- Hook stdin/stdout contract: `HOOK_SCHEMA_VERSION` independent of
  `HTTP_API_VERSION` (ADR-068 D8); one key-pinning test in bk-2.
- Herdr IPC: unchanged.
