# Case Study: atm-graft Leaking Out From Behind Its Own Traits

**Sources** (verified via `git log --all --grep`):
- `4eb30a25` — "feat(phase-U): remove T.6/T.7/T.8 graft integration" (full revert)
- `609bcc0d` — "T.7: daemon graft runtime — registration, bounded nudge queue, drain/fetch API (#239)" (the code that got reverted)
- `4ba0e002` — "feat: land U.9 client-owned graft runtime" (the redesign that replaced it)
- `.triage/phase-T/findings/ATM-QA-T7-008.ttl` and `RSH-T7-001.ttl` (a specific
  symptom finding on the reverted design, confirmed open on both T.7 and T.8)
- `.triage/phase-T/findings/ATM-QA-BOUNDARY-001.ttl` (a related, narrower
  rusqlite leak that recurred specifically on the graft-client-surface
  branch — cross-referenced from
  `docs/architecture/boundary/rusqlite-storage-coupling.md`, not restated
  here)

Citations to the reverted `crates/atm-daemon/src/graft_runtime.rs` are read
directly from the historical commit (`git show 609bcc0d:...`) since the file
no longer exists at HEAD — verified against that specific commit blob, not
approximate.

**Evidence legend**: **verified** = directly re-read from commit/blob
content in this review pass; **triage-sourced** = quoted from TTL
occurrence entries without independent re-read; **approximate** = inferred
from commit diff/history rather than an exact citation.

## (a) What boundary was supposed to exist

atm-graft's own design intent (per the later, corrected commit message) is
that the daemon should have **no graft-specific protocol knowledge**. The
daemon is a generic message-transport host; "graft" (an embedded/session
client integration concept — session lifecycle, nudge delivery, batch
limits) is a client-owned concern that should sit *behind* a thin trait
surface the daemon calls into, not be baked into daemon internals as a
first-class runtime component. The boundary: `atm-daemon` depends on a
narrow port (something like `GraftPostSendPort` / an observability-routed
notification callback); it should never own graft's session/queue state
machine directly.

## (b) How it leaked

### The structural leak: graft became a daemon-internal runtime instead of a client-owned surface behind a port

T.6-T.8 (commits `70bb25d8`, `609bcc0d`, `0094526e`) built `GraftRuntime` as
a stateful component living inside `atm-daemon` itself
(`crates/atm-daemon/src/graft_runtime.rs`), holding session registration,
per-session bounded nudge queues, and drain/fetch RPC handling — i.e., the
daemon's own `RuntimeHealth`/dispatch layer had direct, compiled-in
knowledge of graft session lifecycle and nudge semantics rather than
treating graft as an opaque client integration reachable through one narrow
port.

This was severe enough that it was not QA'd down to a fix — it was fully
reverted. Commit `4eb30a25` ("feat(phase-U): remove T.6/T.7/T.8 graft
integration") states the rationale directly: *"Removes GraftRuntime from
atm-daemon, graft module from atm-core, and the atm-graft crate entirely.
The graft feature was incorrectly integrated into daemon internals; this
reverts the integration so the daemon has no graft-specific protocol
knowledge."* The replacement, `4ba0e002` ("feat: land U.9 client-owned graft
runtime"), rebuilt the capability with a **client-owned primary runtime**
in the `atm-graft` crate plus a new `atm-core::boundary` port
(`crates/atm-core/src/boundary/mod.rs`, +20 lines in the U.9 commit) — but
verified against `git show --stat 4ba0e002` directly, U.9 itself *also*
re-added `crates/atm-daemon/src/graft_runtime.rs` (+510 lines) and modified
`crates/atm-daemon/src/runtime_health.rs` (213 lines changed). Reading that
U.9-vintage `graft_runtime.rs` blob shows it still held a session/queue
state machine (`GraftRuntimeState`, `RegisteredGraftSession` with a
`VecDeque<NudgeEvent>` and `dropped_count`) inside `atm-daemon` — a narrow
daemon-side adapter/port, not the thin pass-through the "client-owned"
framing implies on its own. The daemon-side `graft_runtime.rs` was removed
entirely only later, in `3730257c` ("feat: land U.10 generic advisory
notification surface"). So at U.9 (`4ba0e002`), the design was "client-owned
primary runtime with a daemon-side narrow adapter/port still present," not
yet a fully daemon-code-free client-owned runtime.

### A concrete symptom of the leak while it existed: bypassing the observability port

`ATM-QA-T7-008` (confirmed open on both T.7 `6986027` and T.8 `afbd906`)
documents a specific manifestation: `GraftRuntime` held no reference to
`Arc<dyn DaemonRuntimeObservability>` — the trait boundary meant to carry all
structured operational signals — so when its bounded nudge queue overflowed,
it emitted a tracing macro directly instead of routing the event through
that boundary. Verified in the T.7 commit
(`crates/atm-daemon/src/graft_runtime.rs`,
`enqueue_nudge_for_recipient`, lines ~192-206):

```rust
if session.nudges.len() >= self.max_nudges_per_session {
    session.dropped_count = session.dropped_count.saturating_add(1);
    overflowed = true;
    tracing::debug!(
        session_id = %session_id,
        team = %outcome.team,
        agent = %outcome.agent,
        cap = self.max_nudges_per_session,
        dropped_count = session.dropped_count,
        "graft nudge queue rejected a nudge because the bounded session queue is full"
    );
    continue;
}
```

The triage finding describes this as `tracing::warn!`; the verified T.7 blob
uses `tracing::debug!` at this call site — a minor mismatch, possibly because
the finding was written against the T.8 revision (`afbd906`), which was not
independently re-read in this pass. The structural point stands regardless
of macro level: `GraftRuntime` is composed *into* `RuntimeHealth`, which does
hold the observability arc, but `GraftRuntime` itself bypasses that arc and
talks straight to the global tracing subscriber — the overflow event is
invisible to the health/observability subsystem that is supposed to be the
one place structured signals flow through.

A related finding, `RSH-T7-001`, is the same root cause from the caller's
side: `enqueue_nudge_for_recipient` drops the oldest nudge and returns
`Ok(())`, so the IPC caller cannot distinguish "nudge queued" from "nudge
silently dropped" — the internal bypass of the observability boundary also
meant no signal reached the request boundary either.

## (c) File:line citations

| File | Lines (as of cited commit) | What's there |
|---|---|---|
| `crates/atm-daemon/src/graft_runtime.rs` @ `609bcc0d` | 174-222 | `enqueue_nudge_for_recipient` — daemon-internal session/queue state machine (verified) |
| `crates/atm-daemon/src/graft_runtime.rs` @ `609bcc0d` | ~197-205 | direct `tracing::debug!` call bypassing `DaemonRuntimeObservability` (verified; triage record cites `tracing::warn!` at line 198 against a later revision, not independently re-verified) |
| commit `4eb30a25` | full commit | revert of `GraftRuntime` from `atm-daemon`, `graft` module from `atm-core`, and the `atm-graft` crate — explicit rationale in commit message (verified) |
| commit `4ba0e002` | full commit, notably `crates/atm-core/src/boundary/mod.rs` (+20), `crates/atm-graft/src/lib.rs` (+1702/-...), `crates/atm-daemon/src/graft_runtime.rs` (+510), `crates/atm-daemon/src/runtime_health.rs` (213 changed) | U.9 redesign: primary runtime moved client-owned into `atm-graft`, but a daemon-side `graft_runtime.rs` adapter/port was re-added, not eliminated, at this commit (verified via `git show --stat` and reading the blob directly) |
| commit `3730257c` | full commit | "feat: land U.10 generic advisory notification surface" — the commit that actually removes `crates/atm-daemon/src/graft_runtime.rs` entirely; graft becomes fully daemon-code-free only here, one commit after U.9 (verified — file absent at current HEAD) |

## (d) Why this is a boundary leak and not a legitimate cross-boundary need

A legitimate cross-boundary need would be: the daemon calls a thin port
(e.g., "notify this post-send event happened") and knows nothing about what
graft does with it. What T.6-T.8 built instead was the daemon *owning* the
graft-specific state machine (sessions, bounded queues, drop counters) — the
daemon needed to know graft's internal vocabulary (`GraftSession`,
`NudgeEvent`, batch limits) to do its job, which is precisely the "concrete
implementation detail leaking above the trait meant to hide it" pattern this
agent exists to catch. The `DaemonRuntimeObservability` bypass compounds it:
even within the daemon, the component that should have used the existing
structured-observability boundary instead reached around it to a global
side channel (`tracing::debug!`) — a smaller instance of the same failure
mode (reaching past an abstraction that was sitting right there) at a
different layer.

The evidence that this was a genuine leak and not an acceptable design
trade-off: the project's own response was not a QA fix-round, it was a full
revert and re-architecture (T.6-T.8 → U.9), with the revert commit message
explicitly stating the daemon should have "no graft-specific protocol
knowledge" at all.

## (e) Recommended fix direction (the pattern U.9 started and U.10 completed)

1. **Client-owned primary runtime with a daemon-side narrow adapter/port,
   trending toward zero daemon-side state.** Move the session/queue/nudge
   state machine into the `atm-graft` crate (the client), not `atm-daemon`.
   U.9 (`4ba0e002`) took the first step — client-owned primary runtime plus
   a new `atm-core::boundary` port — while still leaving a daemon-side
   `graft_runtime.rs` adapter in place; U.10 (`3730257c`) finished the job by
   removing that daemon-side file entirely in favor of a generic advisory
   notification surface the daemon calls without knowing graft's internal
   vocabulary.
2. **Route every structured event through the existing observability
   boundary.** Any component with access to `RuntimeHealth`/the daemon
   runtime should receive (or be composed with) the
   `Arc<dyn DaemonRuntimeObservability>` reference explicitly, rather than
   calling global `tracing::*` macros as a side channel. If a component
   can't reach the observability arc, that's a signal it's sitting at the
   wrong layer, not a reason to bypass structured logging.
3. **Surface drops/overflow at the response boundary, not just in logs.**
   Add an explicit field (e.g. `nudge_dropped`) to the response type so
   callers can react programmatically, instead of relying on a
   best-effort trace line that may or may not be observed.
4. **Treat "had to fully revert and re-architect" as the strongest possible
   signal of a real boundary leak** — worth recognizing early rather than
   discovering after a full sprint arc (T.6 → T.8) has already built out the
   daemon-internal version.
