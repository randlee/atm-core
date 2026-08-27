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

This sprint's Herdr behaviour claims are governed by
[ADR-058](docs/adr/ADR-058-herdr-local-steer-backend-contract.md) (`herdr` 0.8.2, derived from source at
`d79fd746`). Where this doc and ADR-058 disagree, ADR-058 is authoritative;
this doc cites it by decision id (`D1`–`D8`).

## Deliverables

1. **Explicit, exclusive local backend selection — complete CLI-to-doctor
   path.** The type is **AQ1's, not this sprint's**
   (`crates/atm-core/src/delivery_channel.rs`, AQ1 blueprint §2.3 `L2.3`) —
   this sprint adds no variant and must not redefine it:

   ```rust
   pub enum LocalMessageReceivedBackend {
       Tmux { pane_id: PaneId },
       Herdr { session: Option<HerdrSession> },  // HerdrSession: validated newtype, AQ1 delivery_channel.rs
   }
   ```

   `session` is the Herdr session the member's agent lives in — roster data,
   exactly like `pane_id`, per ADR-058 D1's "Decision (Rand, 2026-08-26)": the
   daemon never launches Herdr sessions (the external team launcher does), so
   the session an agent lives in must be stored per member. `None` means
   Herdr's default server (`~/.config/herdr/herdr.sock`). This sprint's job is
   the CLI, persistence, and doctor surface for this type — it implements no
   new variant and stores no separate `HerdrAgentTarget`: the live target is
   always the member `AgentName`, resolved by Herdr at nudge time.

   The public command surface is conditional and explicit on **both**
   operations:

   ```text
   atm teams add-member <team> <member> --backend tmux --target %N
   atm teams add-member <team> <member> --backend herdr [--session <name>]
   atm teams update-member <team> <member> --backend tmux --target %N
   atm teams update-member <team> <member> --backend herdr [--session <name>]
   ```

   `--backend tmux` requires `--target`; `--backend herdr` accepts an
   optional `--session <name>` and rejects `--target` (clap/validation error —
   Herdr has no user-configurable target, only a session/server selector).
   `--session` without `--backend herdr` is also a validation error. The
   current `--pane-id` option (`crates/atm/src/commands/teams.rs:63-66`,
   `:93-96`) is retained only as a deprecated compatibility spelling for
   `--backend tmux --target <value>`; it cannot be combined with either new
   option. Help, clap parsing, request DTOs, and errors must describe these
   conditional rules — neither command may expose a tmux-only `Option<String>`
   as its primary path.

   **Identity grammar validation (ADR-058 D1, "Name grammar constraint").**
   Herdr only accepts agent names matching `^[a-z][a-z0-9_-]{0,31}$`
   (`herdr` `src/app/agents.rs:13-20`). At `add-member` **and**
   `update-member` time, whenever `--backend herdr` is selected, validate the
   `<member>` `AgentName` string against that pattern and **reject** (hard
   `AtmError::validation`, not a warning — consistent with every other
   `member_mutation.rs` validation failure) if it does not match, naming the
   pattern in the error. This is strictly narrower than, and independent of,
   `AgentName::from_str`'s existing `validate_path_segment` check
   (`crates/atm-storage/src/types.rs:179-187`) — an `AgentName` can be valid
   ATM-wide (e.g. `Team-Lead`) yet unusable as a Herdr target; the CLI must
   catch that at the point the operator chooses `--backend herdr`, not defer
   it to a runtime `agent_not_found`. `--session <name>` is validated with the
   existing `atm_storage::validation::validate_path_segment(name, "herdr
   session")` (`crates/atm-storage/src/validation.rs:3`) because it becomes a
   `<config_dir>/sessions/<name>/` path segment (ADR-058 D1).

   **Persistence: `metadata_json`, no schema migration.** `RosterEntry` is
   `atm_storage::contract::RosterMember`
   (`crates/atm-core/src/boundary/store.rs:36`); its `recipient_pane_id:
   Option<PaneId>` and `metadata_json: Map<String, Value>` fields
   (`crates/atm-storage/src/contract.rs:459-462`) both already exist, and the
   `team_roster.metadata_json TEXT NOT NULL DEFAULT '{}'` column has already
   been present since an earlier `ensure_column` migration
   (`crates/atm-storage-rusqlite/src/shared_db.rs:75`, `:757-758`). AQ1's
   `local_message_received_backend` derivation (blueprint §2.3) already reads
   `metadata_json["backendType"] == "herdr"` /
   `metadata_json["herdrSession"]`. This sprint therefore adds **no DDL, no
   `ensure_*` migration function, no new column, and no backfill of existing
   rows** — it only extends the two roster-mutation functions in
   `crates/atm-core/src/team_admin/member_mutation.rs`
   (`build_member_add_roster_record` at `:337-364`,
   `apply_member_metadata_update` at `:374-401`) to write
   `metadata_json["backendType"] = "herdr"` plus, only when `--session` was
   given, `metadata_json["herdrSession"] = <name>`, and to clear
   `recipient_pane_id` to `None` on that path. `--backend tmux` continues the
   existing `backendType: "tmux"` + `recipient_pane_id` write unchanged.

   **Frozen (critical review I21 — blast-radius inventory).** `grep -rn
   recipient_pane_id crates` finds ~80 sites across `atm-core` (
   `delivery_policy.rs`, `service_runtime.rs`, `boundary/{mod,store}.rs`,
   `team_admin{.rs,/projection.rs,/restore.rs,/member_mutation.rs}`,
   `doctor/mod.rs`, `read/mod.rs`, `clear/mod.rs`, `list.rs`, `graft.rs`,
   `send/{hook,mod,tests,nudge_template}.rs`), `atm-storage`
   (`contract.rs:460` field), `atm-storage-rusqlite` (`roster_store.rs`,
   `shared_db.rs` DDL/migration, `lib.rs`), `atm` (`commands/{teams,members,
   internal_nudge}.rs`, `composition.rs`), `atm-http-runtime`
   (`storage_and_nudge_router.rs`), `atm-graft`/`atm-graft-python`. Per AQ1 AC
   2 ("compat surfaces proven unchanged... doctor JSON fields unchanged"),
   none of these struct shapes, field names, the DB column, or the
   `MemberSummary.tmux_pane_id` / `AgentMember.tmux_pane_id` /
   `ProjectedRosterEntry.tmux_pane_id` / `NonClaudeOutboundDeliveryRequest
   .recipient_pane_id` projection field names move. A uniform-tmux roster's
   `atm teams members --json` output is byte-identical before/after this
   sprint. Only `member_mutation.rs`'s two functions above, `doctor/mod.rs`
   (new finding), `team_admin/projection.rs` (new **additive** `backend` /
   `herdrSession` JSON fields, `tmux_pane_id` untouched), and `delivery_policy
   .rs::DeliveryRecipientSnapshot::from_roster` (deliverable 2 — a real logic
   fix, not the "visibility only" touch AQ1's file-ownership table assumed,
   because AQ1 has no Herdr routing to make correct) change.

   **Mutual exclusivity is a storage invariant.** `teams update-member` is
   the backend-environment/target-type check path: setting Herdr mode
   atomically clears the member's persisted tmux pane-id (and any stale
   `herdrSession` key when re-selecting `--backend tmux`); setting a tmux pane
   target atomically replaces Herdr mode with `Tmux`. Add follows the same
   one-backend representation. There is never a row, projection, backup entry,
   restore result, or doctor record with both a pane-id and Herdr mode stored;
   update must not append a second target or preserve a stale pane-id or
   stale `herdrSession`. Bare `update-member --backend herdr` (no `--session`)
   on a row that already has a stored session **clears** it — mode-only
   selection is idempotent-explicit, never a partial merge.

   **Tmux-only target parsing, with the M11 warning-path change.**
   `normalize_tmux_pane_id` (`member_mutation.rs:439-455`) remains tmux-only:
   it is invoked only for `--backend tmux --target <value>`; Herdr receives no
   CLI target string at all. Today it hard-rejects (`AtmError::validation`)
   anything that is not `%<digits>` or bare digits, which **disagrees** with
   `PaneId::from_cli` (`crates/atm-storage/src/types.rs:788-799`), which
   already accepts any non-blank string (including tmux's `session:1.2`
   window/pane form — exercised by `team_admin.rs:761`). This is the
   pre-existing "old `PaneId::from_cli` versus `normalize_tmux_pane_id`
   disagreement" this sprint's AC 3 closes, and it is critical review M11's
   flagged reject→accept-with-warning change — stated explicitly, not
   dropped, because AC 3 depends on it (approved by Rand 2026-08-26). New contract: `normalize_tmux_pane_id`
   returns `Result<Option<(PaneId, TmuxTargetShape)>, AtmError>` where
   `TmuxTargetShape` is `Strict` (matches `%<digits>`/bare digits) or
   `NonStandard` (anything else `PaneId::from_cli` still accepts — it still
   rejects only a blank string). The CLI layer treats `Strict` silently and
   `NonStandard` by emitting the warning below; both persist via
   `PaneId::from_cli`. Test: `normalize_tmux_pane_id("session:1.2")` →
   `Ok(Some((PaneId("session:1.2"), NonStandard)))`;
   `normalize_tmux_pane_id("%7")` → `Ok(Some((PaneId("%7"), Strict)))`;
   `normalize_tmux_pane_id(None)` → `Ok(None)` (unchanged); blank string still
   rejected via `PaneId::from_cli`'s own blank check.

   **Doctor roster-consistency finding — Warning, not Error (critical review
   I20).** In the same doctor-threading change, group only members with an
   explicit local backend by team (members with no backend are
   transient/adhoc and are skipped). If a team has at least one `Tmux` member
   **and** at least one `Herdr` member, emit one `DoctorFinding` with
   **`DoctorSeverity::Warning`** (new `AtmErrorCode::RosterMixedLocalBackend`
   / `"ATM_ROSTER_MIXED_LOCAL_BACKEND"` in
   `crates/atm-error/src/error_codes.rs`), naming the team plus its tmux and
   Herdr member lists, with the same conditional remediation
   (`atm teams update-member <member> --backend herdr` or
   `--backend tmux --target %N`). **Decision approved by Rand 2026-08-26. Rationale (why Warning, not the
   originally planned Error):** ADR-058 D1 explicitly anticipates operators
   running a team split across Herdr sessions and, transitively, migrating a
   team's members from tmux to Herdr one at a time — a mixed team is a
   supported transitional state, not a broken one, and each member's own
   backend routes correctly regardless of what its teammates use (per-target
   validation, unchanged from the paragraph below, is the real safety net for
   an actually wrong target). `DoctorStatus::Warning` does **not** trip
   `has_errors()` (`crates/atm-core/src/doctor/report.rs:200-203`) or
   `atm doctor`'s `std::process::exit(1)`
   (`crates/atm/src/commands/doctor.rs:35-39`); the finding still prints and
   still appears in `--json`. A uniform tmux team, a uniform Herdr team, and a
   team with only one explicit backend plus backend-less members produce no
   finding at all.

   **Tmux target-type warning (required, unchanged from the prior draft —
   this is a different check from the mixed-backend finding above and is
   kept as-is per critical review I20's "keep per-target validation").** A
   `NonStandard`-shape `--target` supplied with `--backend tmux` is accepted
   (per the M11 change above) with a warning to stderr and the structured
   command result naming the member, selected backend, and target:
   `verify --backend (herdr|tmux) for every member in team <team>;
   mixed-backend rosters require an explicit correct backend`.
   `--backend herdr --target <value>` is instead the clap/validation error
   from above. The warning is intentionally not silent and not a per-target
   probe: backend ownership is environment-derived, so the CLI cannot prove
   that a non-`%N` pane target belongs to the intended tmux environment.

   **Doctor Herdr-presence probe (deliverable 8 detail, ADR-058 D1's closing
   paragraph).** For each explicit-Herdr member, `atm doctor` additionally
   runs `herdr agent get <AgentName>` (via deliverable 3's shared adapter,
   §`get`) under that member's stored session env, bounded by a short doctor
   probe deadline (2s). `agent_not_found` (or any adapter transport failure)
   produces a **separate**, per-member `DoctorSeverity::Warning` finding
   (`AtmErrorCode::HerdrAgentNotVisible` / `"ATM_HERDR_AGENT_NOT_VISIBLE"`)
   reading exactly "agent not visible in the member's configured Herdr
   session" (ADR-058 D1's own wording), distinct from the mixed-backend
   finding and from each other. This is a live probe, so it degrades to a
   single roll-up Info finding ("Herdr presence probe skipped: `<reason>`")
   rather than failing the whole doctor run when no `herdr` binary is on
   `PATH` or the socket is unreachable — mirrors the existing
   `AtmObservabilityHealth` Healthy/Degraded/Unavailable pattern
   (`crates/atm-core/src/doctor/health.rs:53-99`).

   The resolved delivery policy is total and ordered once: explicit local
   backend (`Tmux` or `Herdr`), otherwise graft lease, otherwise AQ2.5
   bare-CLI. Neither the planner nor emitters may reimplement that match.

2. **Full-parity, sealed delivery seam — one mapping owner (closes critical
   review I19).** AQ1's classifier (`crates/atm-core/src/delivery_channel.rs`)
   already takes `Option<&LocalMessageReceivedBackend>` and a `GraftLeaseState`
   and already returns `DeliveryChannel::HerdrSteer`; this sprint makes that
   channel *deliverable* by supplying the emitter (deliverable 3) — it does
   not widen, redefine, or re-own the classifier (that was the AQ2.5↔AQ2.6
   circular ownership, critical review B5, resolved by moving the seam to
   AQ1). **There is no `delivery_channel()` method anywhere in this design —
   delete that phrase from the prior draft.** `classify_delivery_channel` is
   a free function; AQ1 owns it exclusively.

   A **second, independent mapping already exists in the tree** and must be
   collapsed onto the classifier as part of this deliverable, or Herdr
   silently misroutes: `DeliveryRecipientSnapshot::from_roster`
   (`crates/atm-core/src/delivery_policy.rs:78-99`) computes its own
   `local_tmux_post_send` / `graft_post_send` booleans by re-parsing
   `member.recipient_pane_id` / `metadata_json["backendType"]` independently
   of AQ1's classifier — with today's logic, a Herdr member on a non-Claude
   harness (`graft_post_send = !local_tmux_post_send`) would fall through to
   `graft_post_send = true` and get dispatched to graft, not Herdr. Fix, in
   the same PR: `from_roster` calls `local_message_received_backend(&member)`
   once and derives `local_tmux_post_send` / a new `local_herdr_post_send`
   bool (plus a stored `herdr_session: Option<HerdrSession>`) from its result;
   `graft_post_send` becomes `harness_is_graft_eligible &&
   local_message_received_backend(&member).is_none()`. `build_built_in_dispatch`
   (`crates/atm-core/src/send/hook.rs:17-49`) gains a third arm using
   `local_herdr_post_send` before the `graft_post_send` arm, producing
   `PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Herdr(HerdrNudgeTarget
   { session: herdr_session }))` (deliverable's two-armed payload, below).
   `rebuild_received_hook_dispatch` (AQ1's `nudge_dispatch.rs`) reuses both
   fixed functions unmodified, so the fix also closes the same gap for
   AQ2.7's dispatch-from-message-id path. After this fix,
   `local_message_received_backend` / `classify_delivery_channel` are the
   **only** place roster→backend routing is computed; every other call site
   consumes their result.

   `build_built_in_dispatch` and `ReplacementReceivedHookSelector` operate on
   one backend-neutral local-steer target and the sealed
   `AsyncMessageReceivedHookEmitter` contract; they do not branch on
   `tmux | herdr` string literals, inspect target syntax, or implement a
   fallback — only the enum match above. Tmux/Herdr mechanics (the tmux
   two-Enter sequence, Herdr's live AgentName lookup, argv, and lifecycle
   errors) are known only to their respective emitter implementations. The
   template remains the existing ATM message-received template for tmux and
   graft; the Herdr implementation deliberately uses the fixed mailbox-read
   wake-up text above, so untrusted message text cannot become terminal
   input.

   **Two-armed local-steer payload** (closes the AQ1 blueprint §4 deferred
   item "Renaming `LocalTmuxNudgeTarget` payload"). AQ1 ships
   `PostSendBuiltInTarget::LocalSteer(LocalTmuxNudgeTarget)`
   (`boundary/mod.rs:148-160`, tmux-shaped payload only); this sprint widens
   it:

   ```rust
   pub enum LocalSteerTarget {
       Tmux(LocalTmuxNudgeTarget),
       Herdr(HerdrNudgeTarget),
   }
   pub struct HerdrNudgeTarget {
       /// The member's stored Herdr session (roster data); `None` = Herdr's
       /// default server. The target agent name is `event.recipient` — not
       /// duplicated here.
       pub session: Option<HerdrSession>,
   }
   pub enum PostSendBuiltInTarget {
       LocalSteer(LocalSteerTarget),
       Graft(GraftNudgeTarget),
   }
   ```

   **Source-audit gate (critical review I19, concrete).** Extend AQ1's
   `scripts/check-nudge-taxonomy.py` (new in AQ1 L3.1) with a rule asserting
   the string literal `"backendType"` appears in exactly two files:
   `crates/atm-core/src/team_admin/member_mutation.rs` (write) and
   `crates/atm-core/src/delivery_channel.rs` (read, inside
   `local_message_received_backend`) — any third occurrence fails CI. This
   makes the "exactly one mapping owner" claim mechanically enforced rather
   than aspirational.

3. **Tokio-native Herdr emitter + a named, shared process adapter (closes
   critical review I18).** `HerdrReceivedHook` implements the existing sealed
   `AsyncMessageReceivedHookEmitter` in
   `atm-daemon-bootstrap/src/received_hook_selector.rs`, beside
   `TokioTmuxReceivedHook` (`:117-176`), following the same pattern: no
   private runtime, `spawn_blocking`, shell, or detached child; every command
   and delay is awaited against the inherited `RequestDeadline`.

   **Adapter placement and name — dedicated crate (structural change, Rand
   2026-08-26).** The Herdr process adapter is its own crate,
   `crates/atm-herdr` (precedent: `crates/atm-graft` — a thin embedded
   client crate with a narrow, named dependency set), **not** a module in
   `atm-http-runtime` and **not** `atm-core`. `atm-herdr` depends only on
   `atm-core` (for `HerdrSession`, `AgentName`, `RequestDeadline`, and
   `AtmError`), `tokio`, and `serde_json` — no `atm-storage`, no
   `atm-storage-rusqlite`, no `atm-http-runtime`. Its two dependents are
   exactly `atm-http-runtime` (AQ2.7's poll pump,
   `sprint-AQ2-7-herdr-queue-wake.md` deliverable 1) and
   `atm-daemon-bootstrap` (this sprint's `HerdrReceivedHook`, over a new
   `atm-daemon-bootstrap -> atm-herdr` edge added beside its existing
   `-> atm-http-runtime` edge in `crates/atm-daemon-bootstrap/Cargo.toml`).
   `atm-core` stays tokio-free (its existing async boundary already
   expresses itself as `Pin<Box<dyn Future>>` without needing tokio, e.g.
   `AsyncMessageReceivedHookEmitter`).

   **Deliverables this paragraph owns:**

   - Workspace `Cargo.toml` gains `"crates/atm-herdr"` as a new member
     (alongside the existing `"crates/atm-graft"` row).
   - `crates/atm-herdr/Cargo.toml`: `[dependencies] atm-core, tokio,
     serde_json`; `[features] test-utils = []` gating the fake adapter (the
     test-double point — a fake `HerdrProcessAdapter` implementation
     records calls without spawning a process, satisfying the `[testing]
     forbidden_test_bypasses = ["std::process::Command"]` rule below and
     the equivalent rule in the new boundary manifest).
   - `boundaries/atm-herdr/herdr-process-adapter.toml` (new manifest,
     `boundaries/<owner-crate>/<name>.toml` convention, precedent
     `boundaries/atm-graft/message-received-hook.toml`):
     `owner_package = "atm-herdr"`; `[dependencies] allowed_dependents =
     ["atm-http-runtime", "atm-daemon-bootstrap"]` (exactly the two above);
     `forbidden_edges = ["atm-core -> atm-herdr", "atm-storage -> atm-herdr",
     "atm-storage-rusqlite -> atm-herdr", "atm-herdr -> atm-daemon-bootstrap",
     "atm-herdr -> atm-http-runtime"]`, enforced by the boundaries
     lint's live-Cargo-edge check.
   - `docs/atm-herdr/{requirements.md,architecture.md,boundaries.md}`
     (atm-graft-style crate docs, authored in planning; this sprint updates
     them only if its implementation deviates from what planning already
     recorded). `boundaries.md`'s section is the normative source the
     boundaries lint matches the manifest against.
   - **Source-audit gate (extends the `check-nudge-taxonomy.py` pattern
     already used for `"backendType"` elsewhere in this sprint):** a new
     rule, `herdr_string_containment_gate`, asserting that every literal
     `herdr` argv token, JSON field name (`agent_status`, `agent_blocked`,
     `error.code`, …), and Herdr error-code string appears **only** inside
     `crates/atm-herdr` — any occurrence of an `herdr agent …` argv literal
     or a Herdr `error.code` string constant outside that crate fails CI.
     This is the mechanical enforcement of "one crate owns the Herdr wire
     format," the same pattern the existing `"backendType"`-occurrence rule
     uses for roster backend routing.

   Trait **`HerdrProcessAdapter`**, real implementation
   **`HerdrProcessInvoker`** (the test-double point, `test-utils`-gated as
   above):

   ```rust
   pub trait HerdrProcessAdapter: Send + Sync {
       fn prompt(&self, agent: &AgentName, session: Option<&HerdrSession>, deadline: RequestDeadline)
           -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, AtmError>> + Send + '_>>;
       fn wait(&self, agent: &AgentName, session: Option<&HerdrSession>, until: &[HerdrAgentStatus],
               timeout: Duration, deadline: RequestDeadline)
           -> Pin<Box<dyn Future<Output = Result<HerdrWaitOutcome, AtmError>> + Send + '_>>;
       fn get(&self, agent: &AgentName, session: Option<&HerdrSession>, deadline: RequestDeadline)
           -> Pin<Box<dyn Future<Output = Result<HerdrGetOutcome, AtmError>> + Send + '_>>;
   }
   pub struct HerdrProcessInvoker;
   impl HerdrProcessAdapter for HerdrProcessInvoker { /* tokio::process::Command, ADR-058 D2/D3 argv+parsing */ }
   ```

   `wait` and `get` exist on the trait now — `get` for this sprint's doctor
   probe, `wait` because ADR-058 D2 documents `agent wait`'s argv as part of
   the Herdr contract this crate owns. **`wait` is not called by any sprint
   in Phase AQ** (ADR-058 D2: documented, not emitted): AQ2.7's poll-based
   queue pump (`sprint-AQ2-7-herdr-queue-wake.md`, rewritten 2026-08-26)
   uses `agent list` instead and adds a fourth trait method, `list`, in its
   own deliverable 1 (this sprint's PR does not add it). `HerdrReceivedHook`
   itself calls only `prompt`. Keeping `wait` on the trait rather than
   deleting it preserves the ADR-058 D2 argv contract as a compiled,
   documented shape even though nothing in this phase invokes it.

   `HerdrProcessInvoker` implements ADR-058 D2's exact argv, one `execve`
   per call, no shell:

   - `prompt`: `herdr agent prompt <AgentName> "You have unread ATM messages.
     Run: atm read"` — no `--wait`.
   - `wait`: `herdr agent wait <AgentName> --until idle --until done --until
     blocked --timeout <ms>` (AQ2.7 only; spelled out per ADR-058 D2, not
     relying on Herdr's default set).
   - `get`: `herdr agent get <AgentName>` (doctor only, ADR-058 D1).
   - **Environment**: `HERDR_SESSION=<session>` is set on the **child
     process only**, and only when `session` is `Some` — the daemon's own
     environment is never read or synthesised into a session name (ADR-058
     D1 "Decision"). `HERDR_SOCKET_PATH`, if present in the daemon's own
     environment, passes through unchanged (inherited, not overridden).
   - Parses exactly one JSON line from stdout on exit 0 or from stderr's
     `error.code` on exit 1 (ADR-058 D3); exit 2 (usage error, no JSON) is
     treated as an atm-core bug (argv construction error) and asserted
     impossible by construction in tests, never pattern-matched at runtime.

   The emitter itself:

   - Every emission invokes `herdr agent prompt <AgentName>` and therefore
     looks the agent up live; persisted Herdr mode is never treated as a
     target-exists guarantee.
   - **Error-code contract (ADR-058 D8, Steer/immediate-only column — this
     sprint never calls `agent wait`, so only codes reachable from `prompt`
     apply).** None of these consume retry budget: immediate steer runs
     outside `NudgeMode::Deferred`, so `PendingNudgeStore` is never touched
     on this path (AQ1 §1.4) and there is no attempts counter to spend.

     | `error.code` (or condition) | Structured outcome | Emitted as |
     | --- | --- | --- |
     | `agent_blocked` | `blocked_before_input` | dedicated event `{member, backend=herdr, outcome=blocked_before_input}`; durable mail unaffected |
     | `agent_not_found` | `target_not_present` | pre-emission outcome; normal CLI success + warning (mail already persisted) |
     | `agent_target_ambiguous` | advisory failure, no retry | operator-facing warning naming the ambiguity; no injected input |
     | `agent_not_ready` | advisory failure | warning; not a retry — Steer has no retry mechanism |
     | `empty_agent_prompt` | impossible by construction | test asserts the fixed text is always non-empty |
     | `agent_prompt_failed`, `internal_error`, `server_unavailable` | advisory failure | warning |
     | `server_not_running`, `protocol_mismatch` | advisory failure | warning + health counter `herdr_unavailable` |
     | exit 2 (no JSON) | atm-core bug | impossible-by-construction test, never runtime-matched |

   - a successful process exit means only that prompt submission was
     accepted; it does not mean the message was read, an agent turn
     completed, or an idle state was observed.

4. **atm-herdr crate internals: `HerdrError` and `HerdrSpawnBreaker`
   (HR-CORE-008, HR-CORE-009, HR-SAFE-005, HR-SAFE-006, HR-TEST-005).**
   Same `crates/atm-herdr` crate and PR as deliverable 3; this is its
   error/breaker/test-double surface, not a separate crate:

   - `HerdrError` (`error.rs`): the closed enum mapping every ADR-058 D8
     stderr `error.code` row this crate parses, plus this crate's own
     transport/timeout/breaker outcomes (`ServerNotRunning`,
     `ProtocolMismatch`, `TimedOut`, `Unavailable { retry_after }`, and an
     `Advisory { code }` catch-all for an unrecognized `error.code`) —
     never matched on `error.message` text or JSON key order
     (`HR-CORE-008`). `From<HerdrError> for atm_core::error::AtmError`
     lets a caller fold it at its own boundary; `atm-herdr` itself never
     constructs an `AtmError`.
   - `HerdrSpawnBreaker` (`breaker.rs`): a per-host, in-memory circuit
     breaker (`HR-CORE-009`) constructed exactly once, at the composition
     root (`atm-daemon-bootstrap::build_replacement_handler`), and shared
     via one `Arc<HerdrSpawnBreaker>` across every `HerdrProcessInvoker`
     call and every member — never a second, independently-constructed
     instance (ADR-058 D10.1). Exponential backoff `1s *
     2^consecutive_failures`, capped at `30s`; half-open state permits
     exactly one probe, whose own outcome decides the next transition.
     Opens only on the infrastructure-class outcomes named in
     `HR-SAFE-005` (`server_not_running`, `protocol_mismatch`, an
     external-timeout kill, or a failed `agent list`/`agent get` call) and
     never on a lifecycle/target-shaped outcome (`agent_blocked`,
     `agent_not_found`, `agent_not_ready`, `agent_target_ambiguous`).
     While open, every adapter method returns `HerdrError::Unavailable
     { retry_after }` without spawning a child (`HR-SAFE-006`). State is
     never persisted to SQLite, the roster, or `.atm.toml`, and resets to
     closed on daemon restart.
   - `testing::FakeHerdrProcessAdapter` (`testing.rs`, `test-utils`
     feature): the sole test double any consumer crate uses below the
     adapter boundary, satisfying deliverable 3's `forbidden_test_bypasses`
     rule and this crate's own equivalent.

5. **Boundary and observability governance.** Update
   `boundaries/atm-core/message-received-hook-emitter.toml`
   (`[status].notes`, currently "the daemon tmux receiver and
   atm_graft::nudge_sink::GraftReceiveHook") and the matching boundary tests
   in the same PR so the permitted implementation inventory names
   `HerdrReceivedHook` as well as the retained tmux/graft implementers. Add
   backend-qualified structured events and health counters for accepted,
   pre-input-blocked, failed, and deadline-cancelled wake-ups. Do not modify
   `sealed`, its visibility, or the trait's crate boundary.

   **Selector match arm extended for Queue-kind Herdr dispatch (consumed by
   AQ2.7).** `ReplacementReceivedHookSelector::select_emitter`
   (`received_hook_selector.rs:85-95`, AQ1 L3.5) ships with `(NudgeKind::Queue,
   _) => None // AQ2/AQ3 own queue-kind emitters` because tmux and graft's
   queue-kind emitters are separately owned by AQ2/AQ3's own pump/sweep code.
   Herdr has no separate queue-kind emitter — AQ2.7 reuses this sprint's
   `HerdrReceivedHook` for its post-claim prompt (same `herdr agent prompt`
   call). This sprint therefore adds the Herdr field and both match arms:

   ```rust
   struct ReplacementReceivedHookSelector {
       tmux: TokioTmuxReceivedHook,
       herdr: HerdrReceivedHook,
       graft: PublishedGraftReceivedHook,
   }
   match (dispatch.kind, &dispatch.target) {
       (NudgeKind::Steer, PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Tmux(_))) => Some(&self.tmux),
       (NudgeKind::Steer, PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Herdr(_))) => Some(&self.herdr),
       (NudgeKind::Steer, PostSendBuiltInTarget::Graft(_)) => Some(&self.graft),
       (NudgeKind::Queue, PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Herdr(_))) => Some(&self.herdr),
       (NudgeKind::Queue, _) => None, // AQ3 owns tmux/graft queue-kind emitters
   }
   ```

   `active_received_hook_selector` (`received_hook_selector.rs:22-29`)
   constructs one `Arc<HerdrProcessInvoker>` (from `atm_herdr`, the new
   dedicated crate — not `atm_http_runtime::herdr_process`, superseded by
   the structural change above) and passes a clone (as `Arc<dyn
   HerdrProcessAdapter>`) into `HerdrReceivedHook::new`;
   `build_replacement_handler` (`atm-daemon-bootstrap/src/lib.rs:179-202`) is
   where AQ2.7's pump receives its own `atm_herdr` adapter instance at
   daemon composition time (see AQ2.7 Dependencies) — both consumers
   construct/receive an `atm-herdr` type over their own existing or newly
   added dependency edge, never through a shared `atm-http-runtime` module.

## Acceptance criteria

1. `teams add-member` and `teams update-member` both round-trip
   `--backend tmux --target %N` and `--backend herdr [--session <name>]`
   through their clap surface, request DTO, persistence (`metadata_json`
   only — no schema change), member list/JSON projection, backup, restore,
   and doctor output. A legacy `--pane-id` input and legacy backup migrate to
   the same explicit `Tmux` record with no behavior change. An update from
   tmux to Herdr clears the tmux pane-id atomically; an update from Herdr to
   tmux replaces Herdr mode (and any stored `herdrSession`) atomically.
   Persistence, projection, backup, restore, and doctor fixtures prove no
   member can retain both a pane-id and Herdr mode. **(deliverable 1)**
2. `--backend herdr` at add-member and update-member rejects an
   `AgentName` outside `^[a-z][a-z0-9_-]{0,31}$` with a grammar-naming error;
   a name accepted by `AgentName::from_str` but rejected by this stricter
   check is exercised explicitly (e.g. `Team-Lead`). `--session <name>`
   round-trips through `validate_path_segment`. **(deliverable 1)**
3. `teams update-member --backend herdr --target <value>` is rejected;
   `--session` without `--backend herdr` is rejected. A `NonStandard`-shape
   `--target` with `--backend tmux` succeeds with the exact roster-wide
   verification warning on stderr and in structured output. Parser tests
   prove the old `PaneId::from_cli` versus `normalize_tmux_pane_id`
   disagreement cannot recur: `normalize_tmux_pane_id` now returns the
   target's shape alongside the normalized `PaneId` and the single tmux
   parser owns that path. **(deliverable 1, M11)**
4. `atm doctor` emits exactly one **Warning**-severity backend-consistency
   finding (`ATM_ROSTER_MIXED_LOCAL_BACKEND`) for a team containing both
   explicit tmux and Herdr members; the finding names the team and both
   member sets and supplies the conditional update-member remediation.
   `atm doctor`'s exit code stays **0** for this finding alone (Warning does
   not trip `has_errors()`); `--json` reports `"severity":"warning"`.
   Members with no backend are skipped: they do not create a finding alone
   or alter the mixed-backend result. A uniform tmux team and a uniform
   Herdr team produce no finding. **(deliverable 1, I20)**
5. `atm doctor` additionally runs the live `herdr agent get <AgentName>`
   presence probe (deliverable 3's adapter) per explicit-Herdr member; a
   not-found result yields a separate per-member Warning finding
   (`ATM_HERDR_AGENT_NOT_VISIBLE`) reading "agent not visible in the
   member's configured Herdr session"; an unreachable Herdr server/binary
   degrades to one roll-up Info finding rather than failing doctor.
   **(deliverable 1)**
6. A migrated tmux roster row selects the unchanged `TokioTmuxReceivedHook`;
   its argv, two-Enter delay, and successful emission path are regression
   tested byte-for-byte against the pre-AQ2.6 behavior. **(deliverable 3)**
7. `DeliveryRecipientSnapshot::from_roster` derives its routing solely from
   `local_message_received_backend`/`classify_delivery_channel` — a fixture
   proves a Herdr member on a non-Claude harness (e.g. `codex-cli`) no
   longer falls through to `graft_post_send = true`. The backend enum reaches
   both `TmuxSteer` and `HerdrSteer` through AQ1's one classifier. The
   generic planner/selector use only the sealed emitter contract and the
   `LocalSteerTarget`/`DeliveryChannel` enums: no delivery-path
   `tmux`-vs-`herdr` string match, target-syntax test, or fallback exists
   outside the two emitter implementations and the classifier itself,
   mechanically enforced by the extended `check-nudge-taxonomy.py`
   `"backendType"`-occurrence rule. A graft lease cannot override an
   explicit local backend, and a backend-less row preserves the existing
   graft/bare-CLI classification. **(deliverable 2, I19)**
8. The exact immediate Herdr argv is `agent prompt`, the member `AgentName`,
   and fixed mailbox-read text — **no `--wait`** — with `HERDR_SESSION` set
   on the child only when the member has a stored session. The send path
   returns after bounded submission/rejection rather than awaiting lifecycle
   settlement; no message body, rendered XML, shell interpolation, tmux,
   raw-key fallback, or persisted Herdr target appears in the Herdr path.
   **(deliverable 3)**
9. An `agent_blocked` fixture proves the command exits with its structured
   rejection before any input reaches the fixture terminal, records
   `blocked_before_input`, and leaves the durable mail readable through
   `atm read`. **(deliverable 3)**
10. An `agent_not_found` fixture proves that live lookup occurs on every
    immediate nudge, records `target_not_present` separately from
    `blocked_before_input`, injects no input, persists the message, and
    returns normal CLI success with the target-not-present warning. Every
    other row of deliverable 3's error-code table is exercised at least
    once and asserted to consume no retry budget. **(deliverable 3)**
11. Deadline/cancellation tests prove no child process or background task
    survives the request; post-commit hook errors remain advisory.
    **(deliverable 3)**
12. A fake implementation of `HerdrProcessAdapter`, gated behind
    `atm-herdr`'s `test-utils` feature (not the real `HerdrProcessInvoker`,
    not `std::process::Command`), is the only test double used to exercise
    `HerdrReceivedHook` and doctor logic below the real adapter boundary,
    per `message-received-hook-emitter.toml`'s `forbidden_test_bypasses`
    and the equivalent rule in `boundaries/atm-herdr/herdr-process-adapter.toml`.
    **(deliverable 3, I18)**
13. A breaker unit-test suite proves: `HerdrSpawnBreaker` opens on
    `server_not_running`, `protocol_mismatch`, an external-timeout kill,
    and a failed `agent list` call; backoff is `1s * 2^consecutive_failures`
    capped at `30s`; half-open state permits exactly one probe; and the
    first successful probe after `retry_after` elapses closes the breaker
    and resets the failure counter. `HerdrError`'s mapping covers every
    ADR-058 D8 `error.code` row. **(deliverable 4)**
14. `select_emitter` routes `(Steer, LocalSteer(Herdr(_)))` **and**
    `(Queue, LocalSteer(Herdr(_)))` to `HerdrReceivedHook`; `(Queue, Tmux(_)
    | Graft(_))` still routes to `None`. **(deliverable 5)**
15. Boundary-manifest freshness for **both** `message-received-hook-emitter.toml`
    and the new `boundaries/atm-herdr/herdr-process-adapter.toml`
    (`allowed_dependents` exactly `["atm-http-runtime", "atm-daemon-bootstrap"]`,
    `forbidden_edges` covering `atm-core`/`atm-storage`/`atm-storage-rusqlite`
    -> `atm-herdr` and `atm-herdr` -> `atm-daemon-bootstrap`/`atm-http-runtime`),
    the `herdr_string_containment_gate` source-audit rule,
    `docs/atm-herdr/{requirements.md,architecture.md,boundaries.md}` present
    and current, `cargo test -p atm-architecture`, and `just test` pass on
    all three lanes. Herdr command fixtures run on macOS/ubuntu; Windows
    verifies selection and command construction only until a supported
    Windows Herdr deployment is explicitly added. **(deliverable 5)**

## Required validation

- A live macOS or Linux Herdr agent receives the mailbox-read prompt while
  idle and while working, then the recipient reads the durable mailbox.
- A live or deterministic blocked-dialog fixture proves `agent_blocked` causes
  no injection and leaves the tmux backend unused.
- Retained tmux and Herdr rows are exercised in the same roster fixture to
  demonstrate coexistence, not migration-by-replacement; the doctor Warning
  finding is observed and `atm doctor`'s exit code is confirmed `0`.
- Update a tmux member with a `NonStandard`-shape target and retain the
  operator-facing roster-wide backend-verification warning; prove that Herdr
  mode with a supplied `--target` is rejected.
- Add or rename a live Herdr agent to its ATM member `AgentName` (launcher
  responsibility — see Dependencies), then prove the prompt uses that name;
  prove a missing name produces immediate success plus the distinct
  target-not-present warning, while queued work enters AQ2.7's held
  target-not-present path.
- Run `atm doctor --json` and human `atm doctor` over a mixed explicit
  tmux/Herdr team plus an unconfigured member: retain the Warning finding's
  member lists/remediation and the zero exit, then prove that a uniform
  backend team and a backend-less-only team do not report it; separately
  exercise the `herdr agent get` presence probe against a live Herdr agent
  and against a stale/absent one.

## Non-closure / out of scope

- Herdr's deferred queue wake policy (AQ2.7).
- Replacing or deleting tmux, changing the tmux confirmation sequence, or
  converting graft/bare-CLI into Herdr.
- New Herdr lifecycle semantics, queue APIs, or turn tracking. The detached
  idle observation is AQ2.7 only; immediate steer never uses `--wait`.
- `herdr agent start` / `herdr agent rename`. atm-core never runs either
  (ADR-058 D6, closing line): the launch convention below is operator/launcher
  documentation only, not code this sprint authors or calls. **Corrected
  example** (the prior draft's `herdr agent start <AgentName> --kind ...`
  omitted the now-required flag): `herdr agent start <AgentName> --kind
  <claude|codex|hermes|...> --pane <workspace_id>:p<N>` — `--pane` is
  mandatory (ADR-058 D6); an existing agent is retargeted with `herdr agent
  rename <TARGET> <AgentName>` instead. Documented in the operator runbook,
  not in atm-core.

## Dependencies

- must_follow: AQ1 (trait foundation: `LocalMessageReceivedBackend`,
  `DeliveryChannel` classifier, sealed emitter extension point, kind-aware
  dispatch, `PendingNudgeStore`). Merge-forward trigger: AQ1 dev push.
  **This sprint is the first implementer of AQ1's seam** (reordered
  2026-08-26 per Rand — Herdr is the phase's most urgent deliverable; the
  former `must_follow AQ2.5` is removed, AQ2.5 now follows this sprint).
- **Dispatch precondition (critical review B9):** [ADR-058](docs/adr/ADR-058-herdr-local-steer-backend-contract.md)
  is merged (PR #1039) — pinned version (herdr 0.8.2 / protocol 20), CLI
  argv including session selection (D1/I16), stderr codes and exit-code
  contract (D3/D8), fixture transcript (`herdr-cli-contract-fixture.md`).
  No dev dispatch before it merges.
- downstream: AQ2.7 consumes this backend's emitter and the **named**
  `HerdrProcessAdapter`/`HerdrProcessInvoker` this sprint delivers in the
  new dedicated `atm-herdr` crate (critical review I18 — resolved: not
  `atm-http-runtime`, and not `atm-core`; `atm-daemon-bootstrap` and
  `atm-http-runtime` each depend on `atm-herdr` directly, so both consumers
  reach the adapter without a shared intermediate module), plus the
  extended `select_emitter` Queue+Herdr arm; see `docs/atm-herdr/
  {requirements.md,architecture.md,boundaries.md}` for the crate's own
  normative surface. **`wait` stays defined on the `HerdrProcessAdapter`
  trait (ADR-058 D2) but AQ2.7's rewritten poll-based pump does not call
  it** — AQ2.7 uses `list` instead (a fourth trait method AQ2.7 itself
  adds, not this sprint). AQ2.5 adds the bare-CLI arm beside the Herdr arm;
  AQ3 adds the skip-Herdr pre-check on drain and sweep.
- parallel_safe: AQ1.5–AQ1.9 (graft registration — disjoint files: this
  sprint touches roster/member-mutation/selector/emitter/`delivery_policy.rs`;
  graft touches `graft.rs`, atm-graft, registration store/routes).
