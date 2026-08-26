# Sprint AQ2.6 — Local Steer Backends: Retained Tmux + Alternate Herdr

Status: draft · Branch: `feature/aq-2-6-herdr-steer-backend` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Add Herdr as a second, explicit local message-received backend. This is an
**alternate implementation**, not a tmux replacement: `TokioTmuxReceivedHook`
and its `tmux send-keys` sequence remain supported and behaviorally unchanged.
Both are selected through the existing sealed
`AsyncMessageReceivedHookEmitter` / `MessageReceivedHookSelector` boundary in
`atm-core`; no new unsealed extension trait and no change to `pub mod sealed`
is permitted (ADR-001 governs the workspace-convention seal).

The backend's job is only to wake a recipient after ATM has durably persisted
mail. It does not carry the message body, decide whether mail is read, or
create a delivery queue. The authoritative delivery instruction is exactly:

```text
herdr agent prompt <AgentName> "You have unread ATM messages. Run: atm read"
```

Herdr performs text-plus-Enter atomically and rejects a target already at an
approval/question UI with `agent_blocked` **before writing any input**. That
rejection is the safety property this backend supplies; it must never fall
back to `agent send-keys`, `pane send-keys`, or tmux. Immediate steer is
fire-and-forget: it submits without `--wait` and never waits for a lifecycle
settlement or a recipient turn. A queued task may remain waiting for an idle
agent for 45 minutes; that detached, long-lived observation belongs only to
AQ2.7's Tokio pump and must never block the send path.

## Deliverables

1. **Explicit, exclusive local backend selection — complete CLI-to-doctor
   path.** Add a roster-owned, validated representation whose local choices
   are:

   ```rust
   pub enum LocalMessageReceivedBackend {
       Tmux { pane_id: PaneId },
       Herdr,
   }
   ```

   Herdr persists **mode only**. Its target is always the member's ATM
   `AgentName`, resolved live by Herdr at nudge time; `HerdrAgentTarget` does
   not exist and no Herdr target is stored, projected, backed up, restored, or
   doctored. This relies on the established launch convention: the Herdr agent
   MUST be started or renamed so its live agent name equals `ATM_IDENTITY`,
   while its workspace equals `ATM_TEAM` — for example,
   `herdr agent start <AgentName> --kind ...` or `herdr agent rename`. The
   resulting `herdr agent prompt <AgentName> ...` resolves the live agent, and
   queue-work not-found/unavailable is handled by AQ2.7's held path rather
   than a stale persisted target.

   The public command surface is conditional and explicit on **both**
   operations:

   ```text
   atm teams add-member <team> <member> --backend tmux --target %N
   atm teams add-member <team> <member> --backend herdr
   atm teams update-member <team> <member> --backend tmux --target %N
   atm teams update-member <team> <member> --backend herdr
   ```

   `--backend tmux` requires `--target`; `--backend herdr` rejects `--target`
   because the `<member>` AgentName is the target. The current `--pane-id`
   option is retained only as a deprecated compatibility spelling for
   `--backend tmux --target <value>`; it cannot be combined with either new
   option. Help, clap parsing, request DTOs, and errors must describe these
   conditional rules — neither command may expose a tmux-only `Option<String>`
   as its primary path.

   **Mutual exclusivity is a storage invariant.** `teams update-member` is
   the backend-environment/target-type check path: setting Herdr mode
   atomically clears the member's persisted tmux pane-id; setting a tmux pane
   target atomically replaces Herdr mode with `Tmux`. Add follows the same
   one-backend representation. There is never a row, projection, backup entry,
   restore result, or doctor record with both a pane-id and Herdr mode stored;
   update must not append a second target or preserve a stale pane-id.

   **Tmux-only target parsing.** `normalize_tmux_pane_id` remains tmux-only:
   it parses `--target` only for `--backend tmux` and returns the typed pane-id
   plus target-shape classification. No unified parser and no Herdr target
   parser exists. A non-`%N` tmux target follows the warning path below; Herdr
   receives no CLI target string at all.

   Replace persisted `recipient_pane_id` with the tagged backend representation
   in member mutation and projection. Existing rows migrate and read as
   `Tmux` with the same pane-id, retaining behavior. Thread the tagged mode
   through add/update persistence, member list/projection and
   JSON output, backup encoding/versioning, restore decoding/migration,
   roster redaction, and `atm doctor --json` / human output in the same PR.
   Retain a derived/deprecated `tmux_pane_id` compatibility field only where
   existing consumers require it; it must never be the source used for
   backend dispatch. Round-trip fixtures cover both variants and a legacy
   backup.

   **Doctor roster-consistency error (required):** in that same doctor
   threading change, group only members with an explicit local backend by team.
   Members with no backend are transient/adhoc and are skipped. If a team has
   at least one `Tmux` member **and** at least one `Herdr` member, emit one
   `DoctorFinding` with `DoctorSeverity::Error` (not Warning or Info), a stable
   backend-consistency code, and a message naming the team plus its tmux and
   Herdr member lists. Its remediation names
   either `atm teams update-member <member> --backend herdr` or
   `atm teams update-member <member> --backend tmux --target %N` and directs
   the operator to correct the affected members. The finding must flow
   through the existing `DoctorStatus::Error` / `has_errors()` path so normal
   doctor output and `atm doctor`'s exit status are nonzero. A uniform tmux
   team, a uniform Herdr team, and a team with only one explicit backend plus
   backend-less members produce no mixed-backend finding.

   **Tmux target-type warning (required):** this is the
   `teams update-member` backend-environment/target-type check. A non-`%N`
   target supplied with `--backend tmux` is accepted with a warning to stderr
   (and the structured command result) naming the member, selected backend,
   and target. `--backend herdr --target <value>` is instead a clap/validation
   error: Herdr has no user-configurable target. The warning must say: `verify --backend
   (herdr|tmux) for every member in team <team>; mixed-backend rosters require
   an explicit correct backend`. It is intentionally not silent and not a
   per-target probe: backend ownership is environment-derived, so the CLI
   cannot prove that a non-`%N` pane target belongs to the intended tmux
   environment.

   The resolved delivery policy is total and ordered once: explicit local
   backend (`Tmux` or `Herdr`), otherwise graft lease, otherwise AQ2.5
   bare-CLI. Neither the planner nor emitters may reimplement that match.

2. **Full-parity, sealed delivery seam.** AQ2.5's classifier input widens
   from `pane_id: Option<&str>` to `Option<&LocalMessageReceivedBackend>`, so
   its single central mapping can make `DeliveryChannel::HerdrSteer` reachable
   alongside retained `TmuxSteer`. The CLI, daemon, and SQLite/store paths
   carry that same tagged value with no backend-specific side channel.
   `build_built_in_dispatch` and `ReplacementReceivedHookSelector` operate on
   one backend-neutral local-steer target and the sealed
   `AsyncMessageReceivedHookEmitter` contract; they do not branch on
   `tmux | herdr`, inspect target syntax, or implement a fallback. The
   classifier obtains its channel from the tagged backend's one central
   `delivery_channel()` mapping rather than matching variants itself.
   Tmux/Herdr mechanics (the tmux two-Enter sequence, Herdr's live AgentName
   lookup, argv, and lifecycle errors) are known only to their respective
   emitter implementations. The template remains the existing ATM
   message-received template for tmux and graft; the Herdr implementation
   deliberately uses the fixed mailbox-read wake-up text above, so untrusted
   message text cannot become terminal input. A source-audit gate rejects
   direct delivery-path tmux-vs-Herdr matches outside the central backend
   categorization/persistence seam and the designated emitter implementations.

3. **Tokio-native Herdr emitter.** `HerdrReceivedHook` implements the
   existing sealed `AsyncMessageReceivedHookEmitter` in
   `atm-daemon-bootstrap/src/received_hook_selector.rs`. It invokes `herdr`
   with separate argv values, awaits the child with the inherited
   `RequestDeadline`, and terminates/reaps it when cancellation drops the
   future. It does not use a private runtime, `spawn_blocking`, a shell, or a
   detached child. The implementation parses Herdr's structured stderr error:

   - Every emission invokes `herdr agent prompt <AgentName>` and therefore
     looks the agent up live; persisted Herdr mode is never treated as a
     target-exists guarantee.
   - `agent_blocked` becomes a dedicated, structured advisory outcome with
     `{member, backend=herdr, outcome=blocked_before_input}`. No byte was
     injected and no alternate backend is tried.
   - Herdr's structured `agent_not_found` / agent-not-running result becomes
     the distinct `{member, backend=herdr, outcome=target_not_present}`
     pre-emission outcome. It is not `agent_blocked`: no live agent existed,
     no input was attempted, and no nudge or queue dispatch occurs. The mail
     has already persisted, so the caller receives normal success plus a
     warning that the Herdr target (the member `AgentName`) is not present.
   - start/timeout/protocol errors are distinct advisory outcomes; durable ATM
     persistence remains successful, matching the existing post-commit hook
     contract.
   - a successful process exit means only that prompt submission was accepted;
     it does not mean the message was read, an agent turn completed, or an
     idle state was observed.

4. **Boundary and observability governance.** Update
   `boundaries/atm-core/message-received-hook-emitter.toml` and the matching
   boundary tests in the same PR so the permitted implementation inventory
   names `HerdrReceivedHook` as well as the retained tmux/graft implementers.
   Add backend-qualified structured events and health counters for accepted,
   pre-input-blocked, failed, and deadline-cancelled wake-ups. Do not modify
   `sealed`, its visibility, or the trait's crate boundary.

## Acceptance criteria

1. `teams add-member` and `teams update-member` both round-trip
   `--backend tmux --target %N` and mode-only `--backend herdr` through their
   clap surface,
   request DTO, persistence, member list/JSON projection, backup, restore,
   and doctor output. A legacy `--pane-id` input and legacy backup migrate to
   the same explicit `Tmux` record with no behavior change. An update from
   tmux to Herdr clears the tmux pane-id atomically; an update from Herdr to
   tmux replaces Herdr mode atomically. Persistence, projection, backup,
   restore, and doctor fixtures prove no member can retain both a pane-id and
   Herdr mode, and no Herdr target field is present anywhere.
2. The launch fixture starts or renames the Herdr agent to the member's exact
   `AgentName` under its `ATM_TEAM` workspace. Herdr mode persists no target;
   the emitter invokes Herdr with that AgentName and an unavailable live agent
   is a distinct immediate `target_not_present` advisory warning; queued work
   follows AQ2.7's held target-not-present path rather than any persisted-target
   retry.
3. `teams update-member --backend herdr --target <value>` is rejected. A
   non-`%N` `--target` with `--backend tmux` succeeds with the exact
   roster-wide verification warning on stderr and in structured output. Parser
   tests prove the old `PaneId::from_cli` versus `normalize_tmux_pane_id`
   disagreement cannot recur because the single tmux parser owns that path.
4. `atm doctor` emits exactly one Error-severity backend-consistency finding
   for a team containing both explicit tmux and Herdr members; the finding
   names the team and both member sets, supplies the conditional update-member
   remediation, sets `DoctorStatus::Error` / `has_errors()`, and makes the
   command exit nonzero. Members with no backend are skipped: they do not
   create an error alone or alter the mixed-backend result.
5. A migrated tmux roster row selects the unchanged `TokioTmuxReceivedHook`;
   its argv, two-Enter delay, and successful emission path are regression
   tested byte-for-byte against the pre-AQ2.6 behavior.
6. The backend enum reaches both `TmuxSteer` and `HerdrSteer` through AQ2.5's
   one classifier. The generic planner/selector use only the sealed emitter
   contract: no delivery-path tmux-vs-Herdr branch, target-syntax test, or
   fallback exists outside the two emitter implementations. A graft lease
   cannot override an explicit local backend, and a backend-less row preserves
   the existing graft/bare-CLI classification.
7. The exact immediate Herdr argv is `agent prompt`, the member `AgentName`,
   and fixed mailbox-read text — **no `--wait`**. The send path returns after
   bounded submission/rejection rather than awaiting lifecycle settlement; no
   message body, rendered XML, shell interpolation, tmux, raw-key fallback,
   or persisted Herdr target appears in the Herdr path.
8. An `agent_blocked` fixture proves the command exits with its structured
   rejection before any input reaches the fixture terminal, records
   `blocked_before_input`, and leaves the durable mail readable through
   `atm read`.
9. An `agent_not_found` / agent-not-running fixture proves that live lookup
   occurs on every immediate nudge, records `target_not_present` separately
   from `blocked_before_input`, injects no input, persists the message, and
   returns normal CLI success with the target-not-present warning.
10. Deadline/cancellation tests prove no child process or background task
   survives the request; post-commit hook errors remain advisory.
11. Boundary-manifest freshness, `cargo test -p atm-architecture`, and
   `just test` pass on all three lanes. Herdr command fixtures run on
   macOS/ubuntu; Windows verifies selection and command construction only
   until a supported Windows Herdr deployment is explicitly added.

## Required validation

- A live macOS or Linux Herdr agent receives the mailbox-read prompt while
  idle and while working, then the recipient reads the durable mailbox.
- A live or deterministic blocked-dialog fixture proves `agent_blocked` causes
  no injection and leaves the tmux backend unused.
- Retained tmux and Herdr rows are exercised in the same roster fixture to
  demonstrate coexistence, not migration-by-replacement.
- Update a tmux member with a non-`%N` target and retain the operator-facing
  roster-wide backend-verification warning; prove that Herdr mode with a
  supplied `--target` is rejected.
- Start/rename a live Herdr agent to its ATM member `AgentName`, then prove the
  prompt uses that name; prove a missing name produces immediate success plus
  the distinct target-not-present warning, while queued work enters AQ2.7's
  held target-not-present path.
- Run `atm doctor --json` and human `atm doctor` over a mixed explicit
  tmux/Herdr team plus an unconfigured member: retain the Error finding's
  member lists/remediation and the nonzero exit, then prove that a uniform
  backend team and a backend-less-only team do not report it.

## Non-closure / out of scope

- Herdr's deferred queue wake policy (AQ2.7).
- Replacing or deleting tmux, changing the tmux confirmation sequence, or
  converting graft/bare-CLI into Herdr.
- New Herdr lifecycle semantics, queue APIs, or turn tracking. The detached
  idle observation is AQ2.7 only; immediate steer never uses `--wait`.

## Dependencies

- must_follow: AQ1 (kind-aware dispatch and `PendingNudgeStore` taxonomy).
- must_follow: AQ2.5 (it owns the initial classifier/target/selector seam;
  AQ2.6 extends it only after AQ2.5 lands). Merge-forward trigger: AQ2.5 dev
  push.
- downstream: AQ3 updates its pre-check to skip `HerdrSteer`; AQ2.7 consumes
  this backend's command adapter for deferred queue wake-ups.
- parallel_safe: none claimed; this sprint touches the selector and planner
  seams after AQ2.5.
