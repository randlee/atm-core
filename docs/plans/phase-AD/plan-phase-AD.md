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
- `.atm.toml` still configures `[[atm.post_send_hooks]]`, but the live daemon
  path can complete a send with no nudge and no warning
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
- `.atm.toml` currently contains a `team-lead` post-send hook rule, but a live
  send produced neither the expected nudge nor a sender-visible warning
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

## Phase-End Follow-Up Review Blockers

Two independent phase-end reviews on `integrate/phase-AD @ 477c3cef` found
that the accepted line is still not release-ready even after `AD.22`.

The authoritative follow-up blockers are:

- the accepted post-send boundary artifacts claim `PostSendHookEmitter` /
  `GraftPostSendPort` are the live seams, but the reviewed implementation still
  bypasses them on the production send path
- mixed-success external hook execution still collapses into incorrect
  sender-visible behavior because matched/succeeded/failed outcomes are not
  tracked distinctly
- built-in template override rows still hide a fourth, undocumented state
  where an empty string disables built-in nudge with no supported reset path
- built-in template override resolution still happens inside
  `atm internal-nudge` instead of upstream of the renderer/delivery step
- `atm-graft` still carries a real timing race in test mode because host nudge
  injection uses a shortened `#[cfg(test)]` deadline rather than deterministic
  readiness
- the accepted line still lacks one authoritative smoke/service-hardening lane
  that proves the repaired post-send matrix and the remaining Windows daemon
  depth cases together
- four closure-artifact obligations from earlier AD execution still need one
  named owner on the accepted phase-close path:
  - `AD9-BLANKPANE-001`
  - the phase-AD triage sweep ledger under `.triage/phase-AD/`
  - `ERRDOC-001`
  - the release-facing `CHANGELOG.md` entry covering the `AD.13` through
    `AD.30` corrective line

`Phase AD` therefore extends again with a follow-up line. `AD.25` through
`AD.30` are release-blocking closure sprints for these phase-end findings.
`AD.23` remains reserved outside this worktree, and `AD.24` is the sibling
smoke-harness planning slot consumed by `AD.29` rather than renumbered here.

## Post-AD30 Dogfood Messaging Blockers

The accepted `AD.30` line still leaves three serious dogfood-discovered ATM
messaging defects open:

- `#498` ATM still allows self-addressed messages to be created
- `#499` `atm ack` on a historical self-addressed message can still create a
  replacement self-addressed message instead of terminating the queue item
- `#500` the product still conflates mailbox inspection with mailbox mutation,
  and the read path can still create ack obligations on display instead of
  respecting sender-owned durable message state

These are still Phase `AD` blockers because they violate the same caller
ownership, send/read/ack contract that `AD.1` through `AD.30` were meant to
restore. Phase `AD` therefore extends again with the `AD.31` through `AD.35`
follow-up line:

- `AD.31` mailbox peek surface and owner-only mutation reset
- `AD.32` durable ack intent and read semantics reset
- `AD.33` self-addressed send rejection
- `AD.34` self-ack loop termination and historical poison cleanup
- `AD.35` messaging protocol and regression closeout

## Follow-Up Closure Artifact Ownership

The `AD.25` through `AD.30` follow-up line splits technical-fix ownership from
phase-close artifact ownership where needed so every lingering review item has
one explicit home:

| Item | Technical owner | Closure-artifact owner | Tracking artifact |
| --- | --- | --- | --- |
| `RULE-001` direct `sc_observability_types` imports in daemon observability helper files | `AD.26` | `AD.26` | `docs/plans/phase-AD/sprint-AD26.md` plus `docs/adr/ADR-020-rule001-observability-adapter-exception.md` |
| `AD9-BLANKPANE-001` validated-on-entry blank pane drift for `team-lead` / `arch-ctm` | `AD.9` | `AD.30` | `.triage/phase-AD/direct-fix-track.md` plus `docs/plans/phase-AD/readiness.md` |
| `ERRDOC-001` member/team-admin error-code closure evidence | `AD.9` | `AD.30` | `.triage/phase-AD/direct-fix-track.md` plus `docs/plans/phase-AD/readiness.md` |
| historical `FTQ-001` env-race record reconciliation | accepted-line code fix predates this follow-up | `AD.30` | `.triage/phase-AD/direct-fix-track.md` plus `docs/plans/phase-AD/readiness.md` |
| phase-AD triage sweep ledger | `AD.30` | `AD.30` | `.triage/phase-AD/direct-fix-track.md` |
| release-facing `CHANGELOG.md` entry for `AD.13` through `AD.30` | `AD.30` | `AD.30` | `CHANGELOG.md` plus `docs/plans/phase-AD/readiness.md` |
| intermediate `AD.25` through `AD.30` closeout record | `AD.30` | `AD.30` | `docs/plans/phase-AD/readiness.md` |
| final phase-close verdict artifact for the messaging follow-up line | `AD.35` | `AD.35` | `docs/plans/phase-AD/readiness.md` |

Notes:

- `AD.29` feeds the authoritative post-send smoke evidence into the
  `AD.30` closeout record for that sub-line, but `AD.35` is the only sprint
  allowed to author the final Phase `AD` readiness verdict once the messaging
  follow-up line is complete.
- `FTQ-001` is a historical phase-`Xb` discovery record. If the accepted line
  keeps that historical TTL open as snapshot provenance, `AD.30` must record
  the reason explicitly in the readiness/direct-fix artifacts so the code fix
  and the historical ledger are not left silently inconsistent.

## Design Rules

Phase `AD` is corrective simplification, not a feature-expansion line.

The governing rules are:

- caller identity and caller team are mandatory only for retained ATM commands
  that require caller-owned state or routing context
- mutating message/mailbox commands act only as the resolved caller identity
- ATM does not implement a special impersonation exception when
  `ATM_IDENTITY` is unset; if the caller is unresolved, mutating commands fail
  closed
- inspection-only commands may inspect another member's queue state, but they
  must not mutate seen state, pending-ack state, or acknowledgement state
- the accepted mandatory caller-context inventory for this rule is:
  - `send`
  - `read`
  - `peek`
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
- `atm peek` is inspection-only, but it is still caller-context resolver
  required rather than resolver-exempt:
  - it shares the retained mailbox query surface with `list` / `read`
  - it may inspect another member with `--as`, so actor/team overrides must be
    validated through the shared CLI-owned resolver instead of command-local
    parsing
  - unlike `doctor`, it is not an identity-free diagnostic exception
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
- `atm peek` is the accepted explicit non-mutating mailbox inspection command
- `atm read` remains the owner-only mutating mailbox-read command
- `atm read --no-mark` is not an accepted long-term surface; inspection and
  mutation must be split into different commands
- message persistence is the send success boundary
- self-addressed messages (`from == to` within the same team) are invalid ATM
  input and must be rejected before persistence
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
- `atm read` and `atm peek` must never create new ack-required state as a side
  effect of display
- sender-owned durable message data, not display-time mutation, decides whether
  a message requires acknowledgement
- active `tmux_pane_id` already exists in SQLite roster state and must remain
  authoritative there rather than drifting back to repo config assumptions
- pane metadata must be settable and repairable from the CLI
- `ReconcileRuntime` and the watched-source daemon lane are not part of the
  accepted Claude Code runtime and must be removed
- daemon notification delivery is not a protected subsystem; if notification
  logging remains, ATM should append directly at the event site instead of
  routing through a retained queue/worker service
- Claude Code no longer uses the Claude backend, so `atm-storage-claude` and
  Claude inbox-append runtime behavior must be removed from the accepted line
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
- any new sealed `atm-core` boundary trait introduced in this phase must land
  with a matching `boundaries/atm-core/*.toml` governance record and a
  `docs/atm-core/boundaries.md` inventory entry before implementation
  dependents close
- every sprint that changes accepted requirements, ADRs, protocol docs, or
  boundary inventories must list those exact documents in its `Exact Targets`
  so review does not depend on downstream prompt reconstruction

- the accepted post-send seam remains attempt-only:
  - caller-owned send/ack logic resolves matching external hooks, built-in
    fallback eligibility, and the concrete recipient target before invoking
    `PostSendHookEmitter`
  - `PostSendHookEmitter` performs the built-in recipient emission attempt only
    and reports typed success/failure back to the caller-owned send/ack path
  - `GraftPostSendPort` is the receiver-specific leaf handoff used only when
    the chosen built-in target is graft-backed

## Scope Rules

Phase `AD` may:

- fix caller context ownership on the full retained ATM command surface
- split mailbox inspection from mailbox mutation by introducing `atm peek` as
  the explicit non-mutating mailbox-inspection command while keeping `atm
  read` owner-only and mutating
- make caller identity and caller team transport explicit and required for
  daemon-backed caller-owned commands
- make local retained command entry points consume the same shared
  caller-context resolver rather than carrying duplicate command-specific
  fallback logic
- keep one shared caller-context resolver with explicit modes:
  - owner-only mutation for mutating commands
  - inspection-mode override resolution for `peek` / `list`
- preserve diagnostic commands that do not need caller identity; `doctor`
  remains identity-free with optional team scoping
- simplify the post-send nudge path to one post-commit emission seam
- restore a shipped built-in post-send nudge path for normal ATM installs
- allow bounded built-in nudge template overrides without requiring teams to
  copy repo-local scripts across many repos
- make built-in nudge override lifecycle explicit with first-class override,
  disable, and reset-to-default semantics instead of hidden empty-string state
- make the accepted post-send seams real on the production send/ack path,
  including mixed-success hook accounting that preserves successful emission
  even when a sibling matching rule warns or fails
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
- add one authoritative end-to-end smoke/service-hardening lane for the
  repaired Phase AD post-send matrix without duplicating the sibling harness
  planning scope
- restore the missing Windows daemon local-IPC integration-depth coverage in
  CI for dispatcher panic during shutdown, accept-error handling, and
  post-terminate connection rejection
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

pub enum PostSendBuiltInTarget {
    LocalTmux(LocalTmuxNudgeTarget),
    Graft(GraftNudgeTarget),
}

pub struct BuiltInPostSendDispatch {
    pub event: PostSendHookEvent,
    pub target: PostSendBuiltInTarget,
}

pub trait GraftPostSendPort: sealed::Sealed + Send + Sync {
    fn deliver_post_send(
        &self,
        event: &PostSendHookEvent,
        target: &GraftNudgeTarget,
    ) -> Result<(), AtmError>;
}

pub trait PostSendHookEmitter: sealed::Sealed + Send + Sync {
    fn emit_post_send(
        &self,
        dispatch: &BuiltInPostSendDispatch,
    ) -> Result<PostSendEmissionPath, AtmError>;
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
  - if recipient has post-send hook capability, caller-owned send/ack logic
    resolves external-hook matches plus any built-in fallback target before it
    calls `emit_post_send(...)`
  - if `emit_post_send(...)` fails, log it and append a sender-visible warning
  - `AD.6` owns the stable post-send emission failure warning/error code used
    by both local-tmux and graft emitters; earlier sprints may reference the
    warning behavior, but they must not invent competing codes for the same
    failure class
  - the deferred `AD18/ARCH-004` RULE-001 scope ruling is now explicit on this
    follow-up line as a library-internal adapter exception:
    - `crates/atm-daemon/src/daemon_runtime_observability.rs` is a real
      library module declared from `lib.rs`, not a binary-internal file
    - that module is the only sanctioned non-`main.rs` daemon source file
      allowed to import `sc_observability_types::{ActionName, OutcomeLabel}`
      directly
    - `AD.26` makes the exception achievable by exporting crate-visible daemon
      aliases or constructor helpers from that module and routing
      `runtime_sqlite_observer.rs` plus `test_observability.rs` through them
    - `ADR-020` plus `.just/lint_boundaries.py` own the formal ruling and CI
      enforcement for this exception
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
    match post_send_hook_emitter.emit_post_send(&dispatch) {
        Ok(path) => append_notification_log(&event, path)?,
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
- quality review trails implementation and must not be used to pause
  downstream sprint development
- before starting work on a sprint branch/worktree, merge forward from the
  latest preceding sprint branch chain already in flight
- if multiple predecessor sprint branches exist in front of the current
  sprint, merge the full predecessor chain before starting new work on the
  current sprint
- sprint branches must merge forward numerically:
  - `AD.1 -> AD.2 -> AD.3 -> AD.4 -> AD.5 -> AD.6 -> AD.7 -> AD.8 -> AD.9 -> AD.10 -> AD.11 -> AD.12 -> AD.13 -> AD.14 -> AD.15 -> AD.16 -> AD.17 -> AD.18 -> AD.19 -> AD.20 -> AD.21 -> AD.22 -> AD.25 -> AD.26 -> AD.27 -> AD.28 -> AD.29 -> AD.30`
- do not stop downstream development waiting for prior sprint QA to pass
- do not run pairwise cross-merges between unrelated `AD` sprint branches
- `AD.24` is planned in a sibling smoke-test worktree and is consumed by
  `AD.29`; do not renumber or alias it inside this follow-up line

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
23. [AD.25 Built-In Nudge Override Lifecycle And Reset Semantics](./sprint-AD25.md)
24. [AD.26 Post-Send Boundary Wiring And Hook Accounting Repair](./sprint-AD26.md)
25. [AD.27 Upstream Built-In Template Resolution Extraction](./sprint-AD27.md)
26. [AD.28 `atm-graft` Host-Nudge Deadline Race Hardening](./sprint-AD28.md)
27. [AD.29 Phase AD Post-Send Smoke Matrix Closeout](./sprint-AD29.md)
28. [AD.30 Windows Daemon Integration-Depth Coverage Closeout](./sprint-AD30.md)
29. [AD.31 Mailbox Peek Surface And Owner-Only Mutation Reset](./sprint-AD31.md)
30. [AD.32 Durable Ack Intent And Read Semantics Reset](./sprint-AD32.md)
31. [AD.33 Self-Addressed Send Rejection](./sprint-AD33.md)
32. [AD.34 Self-Ack Loop Termination And Historical Poison Cleanup](./sprint-AD34.md)
33. [AD.35 Messaging Protocol And Regression Closeout](./sprint-AD35.md)

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
- built-in template override lifecycle is explicit:
  - non-empty override rows replace the product default
  - explicit disable is distinct from explicit reset-to-default
  - empty-string rows are rejected rather than interpreted implicitly
- repo config no longer carries obsolete `[atm].identity`
- post-send configured recipients either receive an emitted nudge or return a
  sender-visible warning
- post-send hook accounting tracks matched/succeeded/failed execution
  distinctly and does not erase successful emission because another matching
  rule warned or failed
- `PostSendHookEmitter` has a machine-readable boundary TOML plus a matching
  `docs/atm-core/boundaries.md` inventory entry, and `AD.11` readiness/lint
  checks fail closed if either record is missing
- `PostSendHookEmitter` and `GraftPostSendPort` are both live runtime seams on
  the accepted implementation path rather than dead governance records around a
  subprocess bypass
- caller-owned send/ack logic keeps external-hook matching, built-in fallback
  selection, and warning/log policy outside `PostSendHookEmitter`; the emitter
  remains an attempt-only built-in delivery seam
- the accepted send path no longer uses `std::process::Command` subprocess
  spawn as the production tmux/graft post-send delivery mechanism
- built-in template override resolution happens upstream of
  `atm internal-nudge`; the renderer/delivery layer does not reopen runtime
  bootstrap composition to re-query override storage
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
- one authoritative Phase AD smoke/service-hardening lane proves:
  - external hook success
  - external hook partial failure
  - built-in fallback
  - override reset-to-default
  - override disable behavior when that state is retained
- `docs/plans/phase-AD/readiness.md` exists on the accepted line and remains
  the sole authoritative closeout artifact for the `AD.25` through `AD.35`
  follow-up line
- `AD.31` through `AD.35` all pass on the accepted line before `Phase AD`
  may close
- `docs/plans/phase-AD/readiness.md` records dual closeout ownership
  correctly:
  - `AD.30` authors the Windows/post-send `AD.25` through `AD.30` sub-line
    closeout record
  - `AD.35` authors the final Phase `AD` verdict after the messaging
    follow-up line is complete
- `.triage/phase-AD/direct-fix-track.md` exists on the accepted line and names
  the closure-artifact owner for the non-code obligations surfaced during plan
  review
- the Windows daemon integration-depth gap from `RSH-AD-END-001` is closed in
  its own sprint and no longer relies on Unix-only local IPC depth coverage
- Windows daemon integration coverage includes the remaining post-restore local
  IPC depth cases for dispatcher panic during shutdown, accept-error injection,
  and post-terminate connection rejection
- Windows `atm-daemon` CI coverage is restored on the accepted line and the
  Windows daemon lane is green; targeted manual regression evidence alone is
  not sufficient for Phase `AD` closure while that lane stays disabled
