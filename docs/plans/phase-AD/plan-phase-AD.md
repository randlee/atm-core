---
title: Phase AD Plan
status: active
branch: integrate/phase-AD
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-AD
---

# Phase AD Plan

## Goal

Restore the original ATM runtime model for caller context, send, read, and post-send
nudge behavior:

- every retained ATM command that requires caller context runs with a known
  caller identity and caller team, while `atm doctor` remains the explicit
  identity-free, optional-team diagnostic exception
- `atm send` persists the message to the database
- if the recipient exposes a post-send hook capability, ATM fires it
- post-send emission failure is logged and surfaced as a sender-visible warning
- `atm read` reads from the database

Phase `AD` exists because the current accepted line has drifted away from that
model in several release-blocking ways:

- bare ATM commands in an `arch-ctm` session can resolve as `team-lead`
- bare ATM commands can also resolve or persist under the wrong team when
  caller team falls back from repo-local/default config instead of the
  invoking shell or explicit command-line context
- historical Claude/compatibility UUID message-id support still remains in the
  shared identity model even though the accepted runtime is now ULID-only
- the accepted `1.2.3` release entered Phase `AD` with repo-local
  `[[atm.post_send_hooks]]` dogfood config, but the live daemon path could
  still complete a send with no nudge and no warning
- the current post-send path is obscured by generic delivery/notification
  machinery instead of one direct post-commit emission path
- graft-specific session, queue, and stream concepts have leaked into
  `atm-core`, daemon request dispatch, and the local IPC protocol even though
  graft is only one receiver implementation behind the post-send boundary
- `ReconcileRuntime` and its file-watch/import lane remain in the daemon even
  though Claude Code no longer uses that subsystem
- daemon notification delivery is currently a separate queue/worker subsystem
  whose practical job is appending JSONL events to disk
- obsolete `[atm].identity` config still trips `ATM_WARNING_IDENTITY_DRIFT`
- roster and pane truth still drift away from the accepted SQLite-owned model
- raw `atm` commands can still diverge by worktree/invocation directory,
  producing unreadable compatibility-only sends unless a wrapper forces the
  command to run from the primary repo root
- `atm read --unread` can persist the read mutation but still report the wrong
  message payload and stale unread counts in the returned result
- `atm read --contains` can still miss messages whose full durable body text
  contains the needle when the metadata path only reconstructs summary-level
  text
- the accepted `1.2.3` release still leaves default post-send nudge dependent
  on repo-local scripts and dogfood-only wrapper usage instead of a shipped ATM
  binary path

## Validated Breakage On Entry

- `ATM_IDENTITY=arch-ctm` was present in the active shell, but bare ATM
  commands still resolved as `team-lead` until `--as arch-ctm` was forced
- bare `atm read` on the accepted `1.2.3` release could still resolve as
  `team-lead@atm-dev` even when `ATM_IDENTITY` and `ATM_TEAM` were unset,
  proving that both caller identity and caller team can be guessed today
- the accepted `1.2.3` dogfood repo config contained a `team-lead`
  post-send hook rule, but a live send produced neither the expected nudge nor
  a sender-visible warning
- `atm doctor --team atm-dev` currently reports
  `ATM_WARNING_IDENTITY_DRIFT`
- current doctor output shows blank `tmux_pane_id` values in roster state for
  `team-lead` and `arch-ctm`
- the current shared identity path still accepts or emits UUID-form message
  ids in code/docs that should now be ULID-only after Claude backend
  retirement

## Boundary Reset Entry Finding

The current accepted `AD` sprint line does not fully restore the intended
daemon boundary. The new review artifact
[`violation-inventory.md`](./violation-inventory.md) records the concrete drift:

- daemon request dispatch currently owns graft session registration,
  fetch/drain, and long-lived advisory stream request families
- daemon runtime currently owns graft-specific session state, per-session
  nudge queues, and stream-loop control
- `atm-core` currently exposes graft advisory DTOs and dispatcher methods as
  shared infrastructure even though they are not fundamental ATM semantics
- `atm-daemon-client` currently serializes graft advisory packet families as
  first-class daemon protocol kinds
- current daemon, graft, and protocol docs now bless the leaked
  daemon-owned graft session model

`Phase AD` therefore extends past `AD.11` with a corrective line. `AD.12`
through `AD.20` are not optional cleanup; they are required to reach the
accepted post-send, message-identity, graft, raw CLI runtime-root, and
read-output consistency architecture.

## Design Rules

Phase `AD` is corrective simplification, not a feature-expansion line.

The governing rules are:

- caller identity and caller team are mandatory only for retained ATM commands
  that require caller-owned state or routing context
- the accepted mandatory caller-context inventory for this rule is:
  - `send`
  - `read`
  - `ack`
  - `list`
  - `clear`
  - `log`
  - `members`
  - `teams`
  - `teams add-member`
  - `teams update-member`
  - `teams backup`
  - `teams restore`
- `atm doctor` is diagnostic-only and must not require caller identity; its
  `--team` override remains optional diagnostic scope, not mandatory caller
  context
- caller identity and caller team must be resolved together by one shared
  CLI-owned caller-context resolver; retained ATM commands must not each parse
  `ATM_IDENTITY`, `ATM_TEAM`, or repo config independently
- the only accepted caller-identity sources at the CLI boundary are an explicit
  command-line override when the command supports it or `ATM_IDENTITY` from
  the invoking shell
- the only accepted caller-team sources at the CLI boundary are an explicit
  command-line override when the command supports it or `ATM_TEAM` from the
  invoking shell
- repo-local `[atm].default_team`, obsolete `[atm].identity`, hook files, and
  daemon ambient environment must never be used to guess missing caller
  context
- invocation directory and discovered workspace root are not daemon/socket/db
  selectors; they are only inputs to config ingress, repo/file checks, and
  hook-relative path resolution
- `atm read` output after a durable read-state mutation must remain
  self-consistent: the returned payload and returned counts must describe the
  post-mutation state of the same selected durable message
- if caller identity or caller team is unresolved for a command that requires
  caller context, the CLI must fail the command and must not contact the daemon
- every downstream request DTO for caller-owned daemon-backed commands must
  carry resolved caller identity and resolved caller team as required fields,
  never optional fields
- the daemon must execute caller-owned commands against declared request
  identity and team only and must never consult daemon ambient
  `ATM_IDENTITY` or `ATM_TEAM` to fill missing caller context
- message persistence is the send success boundary
- post-send behavior is a post-commit side effect only
- post-send behavior is event-driven; it is not planned through a generic
  delivery-plan abstraction
- ATM owns post-send emission, emission logging, and sender warnings on
  emission failure
- the shipped default post-send path is the built-in `atm internal-nudge`
  command
- one concrete 1.2.3 release root cause was that `cargo publish` did not ship
  `scripts/atm-nudge.py` or `scripts/atm-nudge.sh`, so any default path that
  still depended on repo-local scripts could not work from an installed binary
- the built-in renderer is bounded to exactly six named template kinds:
  - `delivery`
  - `delivery_ack`
  - `delivery_task`
  - `delivery_task_ack`
  - `acknowledge`
  - `acknowledge_task`
- the accepted built-in compact acknowledge forms are:
  - `<atm kind="ack" from="..." message-id="..."/>`
  - `<atm kind="ack" from="..." message-id="..." task-id="..."/>`
- ATM does not own receiver-side consumption after successful emission
- retained ATM message identity is ULID-only across CLI, daemon, storage,
  schemas, and docs; historical UUID-wire compatibility is retired with the
  Claude backend
- graft and tmux remain receiver implementations behind the post-send boundary;
  daemon/core contracts must not model graft-specific session registration,
  fetch/drain, stream control, or queue semantics as shared infrastructure
- local tmux-backed recipients use `TmuxNudgeSink`
- graft-backed recipients use `GraftNudgeSink`
- `TmuxNudgeSink` preserves the current paste + `Enter` + short sleep +
  second-`Enter` operational path, with the delay treated as an
  implementation-tuning parameter rather than an unspecified side effect
- `atm read` is a database read path only
- active `tmux_pane_id` already exists in SQLite roster state and must remain
  authoritative there rather than drifting back to repo config assumptions
- pane metadata must be settable and repairable from the CLI
- repo-tracked `.atm.toml` is not a live pane-routing authority; committed
  dogfood config must not carry `[[atm.post_send_hooks]]` defaults or
  `[[rmux.windows.panes]].tmux_pane_id` values as production routing truth
- `ReconcileRuntime` and the watched-source daemon lane are not part of the
  accepted Claude Code runtime and must be removed
- daemon notification delivery is not a protected subsystem; if notification
  logging remains, ATM should append directly at the event site instead of
  routing through a retained queue/worker service
- Claude Code no longer uses the Claude backend, so `atm-storage-claude` must
  be removed from the accepted line and Claude inbox-append runtime behavior
  must be removed as a governing runtime path
- if a bounded Claude mailbox compatibility export helper survives during this
  phase, it must be explicit obsolete-only scaffolding and must not participate
  in caller-context ownership, post-send ownership, or durable read semantics
- post-send nudge emission must not depend on Claude inbox append success
- durable message delivery remains allowed, but post-send nudge ownership must
  be narrowed back to one direct seam
- post-send hook capability resolution must not depend on the caller's current
  working directory or on whichever repo-local `.atm.toml` happens to be found
- directory and roster cleanup in this phase must prefer deletion and direct
  field ownership over adding new coordinator structs, planner structs, or
  compatibility-only state machines
- UUID/ULID bridge code, UUID parsing fallback, and UUID-wire-only schema
  variants are compatibility-only state and must be deleted rather than
  carried forward into the accepted line
- if a receiver implementation needs active/inactive runtime state, that state
  belongs behind the receiver-owned capability and must not leak back into the
  daemon dispatcher, shared protocol DTOs, or transport receive loop
- shared backend interoperability remains mandatory through the `atm-storage`
  contract; SQLite is one backend and future SQL backend support remains a
  requirement after the Claude backend is retired
- backend interoperability does not require multiple live concrete backends on
  the accepted line; it requires that the shared contract stays future-backend
  ready without another architectural rewrite
- `AD.17` owns re-enabling the Windows `atm-daemon` CI lane on the accepted
  line and must not close while that lane remains disabled, cancelled, or red
- any new sealed `atm-core` boundary trait introduced in this phase must land
  with a matching `boundaries/atm-core/*.toml` governance record and a
  `docs/atm-core/boundaries.md` inventory entry before implementation
  dependents close
- every sprint that changes accepted requirements, ADRs, protocol docs, or
  boundary inventories must list those exact documents in its `Exact Targets`
  so review does not depend on downstream prompt reconstruction

## Scope Rules

Phase `AD` may:

- fix caller context ownership on the full retained ATM command surface
- make caller identity and caller team transport explicit and required for
  daemon-backed caller-owned commands
- make local retained command entry points consume the same shared
  caller-context resolver rather than carrying duplicate command-specific
  fallback logic
- preserve diagnostic commands that do not need caller identity; `doctor`
  remains identity-free with optional team scoping
- simplify the post-send nudge path to one post-commit emission seam
- restore a shipped built-in post-send nudge path for normal ATM installs
- allow bounded built-in nudge template overrides without requiring teams to
  copy repo-local scripts across many repos
- add or tighten trait contracts for local tmux-backed and graft-backed
  post-send emission
- remove graft-only session registration, fetch/drain, queue, and advisory
  stream concepts from shared `atm-core`, daemon request dispatch, and the
  accepted daemon packet registry
- remove UUID message-id compatibility from shared `atm-core`, storage,
  schema/tooling, and accepted documentation surfaces
- remove `ReconcileRuntime` and the daemon watch/import subsystem
- remove daemon notification queue/worker delivery and replace any retained
  notification logging with direct append logic if the log still has value
- remove the retired Claude backend and obsolete Claude inbox-append runtime
  assumptions while preserving the shared backend contract and future SQL
  backend requirement
- repair roster drift detection and operator guidance
- finish the CLI-managed repair/update path for the existing SQLite-owned pane
  and member-home metadata and remove lingering `.atm.toml` assumptions
- simplify directory metadata ownership so only durable `home_dir`, runtime
  `live_cwd`, and log-only startup `launch_cwd` remain
- remove committed pane-id routing state from repo config and keep live pane
  routing in SQLite roster state plus CLI repair/update flows only
- keep repo-local troubleshooting helpers roster-backed or explicit-override
  only; they must not revive `.atm.toml` pane discovery as a production seam
- add smoke and doctor coverage required to keep these regressions closed

Phase `AD` must not:

- redesign durable message delivery semantics
- make send success depend on downstream nudge consumption
- treat receiver-side consumption as an ATM send failure after successful
  emission
- preserve stale post-send behavior just because it is already threaded through
  `DeliveryPlan`, `NotificationSink`, or advisory queue code
- preserve graft-specific session/stream protocol families in shared daemon or
  transport contracts just because they already exist
- preserve UUID message-id compatibility after Claude backend retirement just
  because conversion code already exists
- keep Claude inbox append as a hidden runtime fallback
- collapse the architecture into a permanent SQLite-only contract
- preserve dead daemon subsystems just because they already exist

## Baseline

- planning branch: `plan/daemon-graft-boundary-reset`
- execution integration branch: `integrate/phase-AD`
- prerequisite accepted line:
  - current `develop` as merged through the accepted `1.2.3` baseline
- active blocking issues:
  - `#421` daemon-mediated identity ownership regression
  - `#440` configured post-send notification can fail silently

## Explicit Contract Sample

The implementation target for post-send emission must stay this direct:

```rust
pub struct PostSendHookEvent {
    pub sender: AgentName,
    pub sender_team: TeamName,
    pub recipient: AgentName,
    pub recipient_team: TeamName,
    pub message_id: AtmMessageId,
    pub description: String,
    pub requires_ack: bool,
    pub is_ack: bool,
    pub task_id: Option<TaskId>,
    pub recipient_pane_id: Option<PaneId>,
}

pub trait PostSendHookEmitter: sealed::Sealed {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
```

Required runtime meaning:

- caller-owned commands:
  - CLI resolves caller identity from explicit override or invoking-shell
    `ATM_IDENTITY`
  - CLI resolves caller team from explicit override or invoking-shell
    `ATM_TEAM`
  - CLI fails locally if caller identity or caller team is unavailable
  - daemon receives caller identity and caller team as required request data
  - daemon never substitutes its own ambient identity or team
- doctor:
  - CLI does not require caller identity
  - CLI accepts optional `--team` diagnostic scope
  - daemon/local doctor paths must not invent or require caller identity
- send:
  - persist
  - if recipient has post-send hook capability, call `emit(...)`
  - if `emit(...)` fails, log it and append a sender-visible warning
  - `AD.6` owns the stable post-send emission failure warning/error code used
    by both local-tmux and graft emitters; earlier sprints may reference the
    warning behavior, but they must not invent competing codes for the same
    failure class
- external post-send compatibility:
  - `ATM_POST_SEND.description` is guaranteed on the retained line
  - `ATM_POST_SEND.task_id` remains present as a string contract for external
    hooks and may be empty when no task is associated
  - optional `to` remains compatibility-only and must not become required for
    the built-in shipped nudge path
  - repo-local `[[atm.post_send_hooks]]` consumers stay supported; any
    dogfood script updates needed for the tightened payload contract land in
    `AD.22`
- read:
  - load from durable state only

If notification logging survives, it should stay equally direct:

```rust
persist_message(...)?;
if recipient_has_post_send_hook {
    match post_send_hook_emitter.emit(&event) {
        Ok(()) => append_notification_log(&event)?,
        Err(error) => {
            log_post_send_failure(&error);
            append_sender_warning(render_post_send_warning(&error));
        }
    }
}
```

No accepted `Phase AD` design should require:

- `DeliveryPlan` construction for post-send emission
- `NotificationSink` for post-send emission
- daemon queue/worker orchestration just to append one notification event
- watched-file reconcile/import machinery to make send or read work
- daemon-owned graft session registration, fetch/drain, or stream packet
  families as part of the shared ATM command/runtime contract
- a `RequestDispatcher` method dedicated to one receiver implementation's
  long-lived stream protocol
- `uuid`-based ATM message identity, UUID parsing fallback, or UUID-wire export
  on retained ATM message paths

## Claude Harness Constraint

Phase `AD` must freeze and enforce this rule:

- Claude inbox JSON append is not part of the accepted Claude Code
  delivery/post-send runtime
- `atm-storage-claude` is retired from the accepted line because Claude Code no
  longer uses that backend
- if a retained Claude mailbox compatibility export helper survives
  temporarily, it is historical compatibility only and must not redefine
  current send/read/post-send semantics
- the shared `atm-storage` contract remains the governing backend seam after
  Claude backend retirement
- any surviving code, tests, docs, or diagrams that still treat inbox append as
  mailbox delivery, nudge delivery, or context injection must be removed or
  rewritten

## Execution Order

Phase `AD` executes the deletion line first so new emitter work does not get
implemented on top of retired Claude JSON, reconcile, or notification
infrastructure.

Phase `AD` orchestration rule:

- `Phase AD` is a strict merge-forward line
- Phase `AD` sprints execute back-to-back without stopping for QA or waiting
  for all prior branches to be green
- `AD.13` through `AD.20` each also have a companion `*-win` execution doc on
  the same branch/worktree as the original sprint
- companion `*-win` docs are Windows-host execution overlays only; they do not
  create new sprint branches, do not widen original sprint scope, and do not
  move closure ownership away from the original sprint branch
- the original sprint doc remains the source of truth for scope, interfaces,
  deletions, deliverables, acceptance, and non-Windows validation; each
  `*-win` doc only adds native Windows execution, Windows-only fix-forward,
  and Windows evidence/reporting requirements
- quality review trails implementation and must not be used to pause
  downstream sprint development
- before starting work on a sprint branch/worktree, merge forward from the
  latest preceding sprint branch chain already in flight
- if multiple predecessor sprint branches exist in front of the current
  sprint, merge the full predecessor chain before starting new work on the
  current sprint
- sprint branches must merge forward numerically:
  - `AD.1 -> AD.2 -> AD.3 -> AD.4 -> AD.5 -> AD.6 -> AD.7 -> AD.8 -> AD.9 -> AD.10 -> AD.11 -> AD.12 -> AD.13 -> AD.14 -> AD.15 -> AD.16 -> AD.17 -> AD.18 -> AD.19 -> AD.20 -> AD.21 -> AD.22`
- do not stop downstream development waiting for prior sprint QA to pass
- do not run pairwise cross-merges between unrelated `AD` sprint branches

1. [AD.1 Caller Context Ownership Restore](./sprint-AD1.md)
2. [AD.2 Obsolete Config Identity Removal And Doctor Contract Repair](./sprint-AD2.md)
3. [AD.3 Claude Backend And Inbox Nudge Retirement](./sprint-AD3.md)
4. [AD.4 Reconcile Runtime Removal](./sprint-AD4.md)
5. [AD.5 Notification Runtime Removal And Post-Send `NotificationSink` Detachment](./sprint-AD5.md)
6. [AD.6 Post-Send Nudge Contract Simplification](./sprint-AD6.md)
7. [AD.7 Local Tmux Post-Send Emitter](./sprint-AD7.md)
8. [AD.8 Graft Post-Send Emitter](./sprint-AD8.md)
9. [AD.9 Update-Member CLI And Roster Repair Path](./sprint-AD9.md)
10. [AD.10 Directory Metadata And Doctor Contract Cleanup](./sprint-AD10.md)
11. [AD.11 Smoke And Readiness Closeout](./sprint-AD11.md)
12. [AD.12 Graft Boundary Reset Planning And Contract Tightening](./sprint-AD12.md)
13. [AD.13 ULID Message Identity Reset](./sprint-AD13.md)
14. [AD.14 Shared Graft Boundary Surface Reset](./sprint-AD14.md)
15. [AD.15 Daemon Advisory Runtime Deletion](./sprint-AD15.md)
16. [AD.16 Thin Graft Receiver Reset](./sprint-AD16.md)
17. [AD.17 Boundary Reset Verification Closeout](./sprint-AD17.md)
18. [AD.18 Raw CLI Runtime Root Unification](./sprint-AD18.md)
19. [AD.19 Read Mutation Output Consistency Repair](./sprint-AD19.md)
20. [AD.20 Read Body-Search Metadata Consistency Repair](./sprint-AD20.md)
21. [AD.21 Built-In Post-Send Nudge And Six-Template Override Surface](./sprint-AD21.md)
22. [AD.22 Nudge Routing State Ownership And Dogfood Transition Cleanup](./sprint-AD22.md)

## Phase Exit Criteria

Phase `AD` closes only when:

- bare daemon-backed ATM commands honor the invoking shell identity correctly
- retained ATM commands that require caller context honor the invoking shell
  identity and team correctly
- retained ATM commands that require caller context fail before command
  execution or daemon dispatch when neither explicit override nor
  invoking-shell `ATM_IDENTITY` / `ATM_TEAM` is present
- `atm doctor` runs without caller identity and accepts optional team scoping
- daemon-backed caller-owned request shapes carry caller identity and caller
  team as required data rather than relying on daemon ambient identity/team
- retained ATM message-id parsing, persistence, API surfaces, and docs are
  ULID-only with no UUID fallback or conversion bridge
- raw retained ATM commands behave identically from the primary repo and from
  sibling worktrees for one `ATM_HOME` / host-home installation, with no
  wrapper-only `cwd` forcing required for correctness
- `atm read` no longer mixes original selection ids with next-unread payloads
  or stale pre-mutation bucket counts after marking a message read
- `atm read --contains` matches both summary text and full durable message
  body text on the accepted metadata path, with no false negative when the
  match appears only in stored body text
- a normal installed ATM binary can emit the default post-send nudge without
  repo-local Python or shell scripts
- the accepted built-in nudge path supports the six named template cases from
  `AD.21`, and template override precedence is explicit rather than implicit
  script sprawl
- repo config no longer carries obsolete `[atm].identity`
- post-send configured recipients either receive an emitted nudge or return a
  sender-visible warning
- `PostSendHookEmitter` has a machine-readable boundary TOML plus a matching
  `docs/atm-core/boundaries.md` inventory entry, and `AD.11` readiness/lint
  checks fail closed if either record is missing
- `ReconcileRuntime`, watched-file import, and daemon reconcile notification
  behavior are removed from the accepted line
- daemon notification queue/worker delivery is removed; any retained
  notification log append is direct
- local tmux-backed members use the approved local emitter
- graft-backed recipients use the approved graft emitter
- the accepted shared ATM contracts no longer expose graft-only advisory
  register/unregister/fetch/drain/stream packet families
- the daemon request dispatcher and transport receive loop no longer own
  graft-specific stream/session behavior
- daemon runtime no longer owns graft-specific session maps or per-session
  nudge queues
- implementation-specific graft receive-loop/session state, if any remains,
  lives entirely inside `atm-graft` and not in `atm-core`, `atm-daemon`, or
  `atm-daemon-client` shared protocol/boundary surfaces
- daemon, graft, and protocol docs no longer describe daemon-owned graft
  advisory queues or dedicated advisory-stream sockets as the accepted design
- `atm-storage-claude` is removed from the accepted line
- the shared backend contract remains intact and documented as future-SQL-ready
- no approved runtime path still treats Claude inbox append as mailbox
  delivery, nudge delivery, or context injection
- active pane-id authority and repair flow runs through the existing SQLite
  roster state plus CLI, not `.atm.toml`
- committed repo config no longer carries stale tmux pane ids as live routing
  state
- the validated-on-entry roster drift for `team-lead` and `arch-ctm` is
  repaired on the accepted line
- any remaining drift category not validated on entry is surfaced with accurate
  diagnostics
- smoke and doctor coverage prove the repaired behavior on the accepted line
- command-matrix coverage proves the repaired caller-context behavior on the
  full retained ATM command surface
- Windows `atm-daemon` CI coverage is restored on the accepted line and the
  Windows daemon lane is green; targeted manual regression evidence alone is
  not sufficient for Phase `AD` closure while that lane stays disabled
