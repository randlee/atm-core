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
herdr agent prompt <agent> "You have unread ATM messages. Run: atm read" --wait
```

Herdr performs text-plus-Enter atomically and rejects a target already at an
approval/question UI with `agent_blocked` **before writing any input**. That
rejection is the safety property this backend supplies; it must never fall
back to `agent send-keys`, `pane send-keys`, or tmux. `--wait` is retained so
the caller observes the settled lifecycle result, but it does not defer prompt
submission or identify a particular turn.

## Deliverables

1. **Explicit, exclusive local backend selection.** Add a roster-owned,
   validated representation whose local choices are:

   ```rust
   pub enum LocalMessageReceivedBackend {
       Tmux { pane_id: PaneId },
       Herdr { target: HerdrAgentTarget },
   }
   ```

   `HerdrAgentTarget` is a validated Herdr live-agent name or pane target;
   it is not inferred from an ATM `AgentName`. Existing persisted
   `recipient_pane_id` rows migrate/read as `Tmux` with the same pane and
   retain their current behavior. A Herdr row requires a target and may not
   ambiguously select a tmux pane. The resolved delivery policy is total and
   ordered once: explicit local backend (`Tmux` or `Herdr`), otherwise graft
   lease, otherwise AQ2.5 bare-CLI. Neither the planner nor emitters may
   reimplement that match. Team-admin add/update/list/backup/restore schemas,
   redaction/doctor output, and compatibility fixtures change together.

2. **Shared planner and selector seam.** AQ2.5's classifier gains
   `DeliveryChannel::HerdrSteer`; its existing `TmuxSteer` arm remains. Core
   gains `PostSendBuiltInTarget::LocalHerdr(HerdrNudgeTarget)` and
   `PostSendEmissionPath::LocalHerdr` beside (not instead of)
   `LocalTmux` / `LocalTmuxNudgeTarget`. `build_built_in_dispatch` selects one
   target from the resolved backend. The template remains the existing ATM
   message-received template for tmux and graft; the Herdr target deliberately
   uses the fixed mailbox-read wake-up text above, so untrusted message text
   cannot become terminal input. `ReplacementReceivedHookSelector` owns one
   `TokioTmuxReceivedHook`, one `HerdrReceivedHook`, and the retained graft
   emitter; each target selects exactly its matching implementation.

3. **Tokio-native Herdr emitter.** `HerdrReceivedHook` implements the
   existing sealed `AsyncMessageReceivedHookEmitter` in
   `atm-daemon-bootstrap/src/received_hook_selector.rs`. It invokes `herdr`
   with separate argv values, awaits the child with the inherited
   `RequestDeadline`, and terminates/reaps it when cancellation drops the
   future. It does not use a private runtime, `spawn_blocking`, a shell, or a
   detached child. The implementation parses Herdr's structured stderr error:

   - `agent_blocked` becomes a dedicated, structured advisory outcome with
     `{member, backend=herdr, outcome=blocked_before_input}`. No byte was
     injected and no alternate backend is tried.
   - start/timeout/protocol errors are distinct advisory outcomes; durable ATM
     persistence remains successful, matching the existing post-commit hook
     contract.
   - a successful prompt is recorded separately from its `--wait` settled
     state. A later settled `blocked` means the submitted turn reached a
     question UI; it is not the pre-injection `agent_blocked` rejection.
   - `unknown` is never treated as idle, done, message-read, or proof of
     completion. The emitter records it only as a returned lifecycle
     observation.

4. **Boundary and observability governance.** Update
   `boundaries/atm-core/message-received-hook-emitter.toml` and the matching
   boundary tests in the same PR so the permitted implementation inventory
   names `HerdrReceivedHook` as well as the retained tmux/graft implementors.
   Add backend-qualified structured events and health counters for accepted,
   pre-input-blocked, failed, and deadline-cancelled wake-ups. Do not modify
   `sealed`, its visibility, or the trait's crate boundary.

## Acceptance criteria

1. A migrated tmux roster row selects the unchanged `TokioTmuxReceivedHook`;
   its argv, two-Enter delay, and successful emission path are regression
   tested byte-for-byte against the pre-AQ2.6 behavior.
2. A Herdr-selected row resolves to `LocalHerdr` and selects only
   `HerdrReceivedHook`; a graft lease cannot override an explicit local
   backend, and a backend-less row preserves the existing graft/bare-CLI
   classification.
3. The exact Herdr argv is `agent prompt`, target, fixed mailbox-read text,
   and `--wait`; no message body, rendered XML, shell interpolation, tmux, or
   raw-key fallback appears in the Herdr path.
4. An `agent_blocked` fixture proves the command exits with its structured
   rejection before any input reaches the fixture terminal, records
   `blocked_before_input`, and leaves the durable mail readable through
   `atm read`.
5. Deadline/cancellation tests prove no child process or background task
   survives the request; post-commit hook errors remain advisory.
6. Boundary-manifest freshness, `cargo test -p atm-architecture`, and
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

## Non-closure / out of scope

- Herdr's deferred queue wake policy (AQ2.7).
- Replacing or deleting tmux, changing the tmux confirmation sequence, or
  converting graft/bare-CLI into Herdr.
- New Herdr lifecycle semantics, queue APIs, turn tracking, or a claim that
  `agent prompt --wait` waits to send.

## Dependencies

- must_follow: AQ1 (kind-aware dispatch and `PendingNudgeStore` taxonomy).
- must_follow: AQ2.5 (it owns the initial classifier/target/selector seam;
  AQ2.6 extends it only after AQ2.5 lands). Merge-forward trigger: AQ2.5 dev
  push.
- downstream: AQ3 updates its pre-check to skip `HerdrSteer`; AQ2.7 consumes
  this backend's command adapter for deferred queue wake-ups.
- parallel_safe: none claimed; this sprint touches the selector and planner
  seams after AQ2.5.
