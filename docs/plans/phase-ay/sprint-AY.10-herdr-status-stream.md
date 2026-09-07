---
id: AY.10
phase: AY
sprint: AY.10
title: Herdr agent-status subscription stream in atm-herdr, behind a trait
branch: feature/ay10-herdr-status-stream
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay10-herdr-status-stream
integration_branch: integrate/phase-ay
stack_parent: AY.8
pr_target: feature/ay8-herdr-socket-transport
target: integrate/phase-ay
status: draft
recommended_agent: cipher
recommended_model: fast
added: 2026-09-06 (subscription socket; see phase-ay-plan.md "Rework record" item 7)
execution_track: socket
parallel_with: [AY.4, AY.5, AY.6, AY.7, AY.9]
dependency_relations:
  - prerequisite: AY.8
    dependent: AY.10
    relation: must_follow
    rationale: the held subscription connection reuses AY.8's endpoint resolution, NDJSON framing, line cap, and fake socket server.
  - prerequisite: AY.9
    dependent: AY.10
    relation: parallel_safe
    rationale: AY.9 owns the daemon-bootstrap composition and doctor files; AY.10 touches only atm-herdr, its fixtures, the architecture pin, and the Herdr requirement docs, and is consumed by nothing until AY.11.
  - prerequisite: AY.10
    dependent: AY.11
    relation: must_follow
    rationale: AY.11 consumes the stream trait this sprint lands.
---

# AY.10 — Herdr agent-status subscription stream in atm-herdr, behind a trait

Add one held Herdr `events.subscribe` connection per configured session that
delivers agent-status changes to atm as they happen, exposed through a new
trait in `atm-herdr`. Nothing consumes it in this sprint. The daemon's
polling behaviour is unchanged.

## Why (Rand, 2026-09-06)

"we shouldn't be creating 50 sockets every 5 seconds"; "I would expect all
agent queries would be done on a single socket."; "yes, I think we need to
add the subscription socket."; "are you proposing we do both and verify
they match. once they match we can drop the polling?"; "this is a prudent
solution."; "and probably lowest risk". This sprint is the "add the
subscription socket" half. AY.11 is the "do both and verify they match"
half. Dropping the poll is AY.12, gated on AY.11's evidence and Rand's
decision.

## Herdr facts this sprint rests on (v0.8.2 9eb52145, `src/api`)

- `events.subscribe` (`schema.rs:231`, `schema/events.rs:12`) takes
  `{"subscriptions":[...]}`, replies `SubscriptionStarted`, then streams one
  JSON line per matching event until the client closes
  (`server.rs` `stream_subscriptions`). It is the only long-lived request
  shape atm uses; every other request stays one-request-per-connection
  (AY.8 C2).
- `Subscription::PaneAgentStatusChanged { pane_id, agent_status:
  Option<AgentStatus> }` (`schema/events.rs:76`) is per pane and per
  status. At subscribe time Herdr probes the pane (`pane_get`,
  `subscriptions.rs` ~205-240) and, when the pane's current status equals
  the subscription's `agent_status`, emits that status as the first event
  on the stream; later it emits only changes matching the filter, deduped
  against the probe (`subscriptions.rs` ~340-410). `AgentStatus` has five
  values: `Idle`, `Working`, `Blocked`, `Done`, `Unknown`
  (`schema/common.rs:154`). Five status-filtered subscriptions per pane
  therefore deliver the pane's current status as the first event on the
  same connection that carries subsequent changes. That is the baseline
  mechanism this sprint uses; no separate `agent.list` snapshot is taken
  for status, so there is no window between a snapshot and the start of
  the stream in which a transition can be lost.
- `pane.created`, `pane.closed`, `pane.updated` (carries full `PaneInfo`
  including `agent_status`, `schema/panes.rs:549`), `pane.exited`, and
  `pane.agent_detected` are subscribable without a pane id.
- The wire `EventEnvelope { event, data }` (`schema/events.rs:362`)
  carries no sequence number. The stream starts at the live sequence with
  no replay (drift table, 20a500a7). A pane created while no subscription
  covering `pane.created` is open is invisible until the next
  `agent.list`; C2 closes that gap by never being without such a
  subscription.
- `agent.list` returns `pane_id` per agent; it is used here only to map
  agent names to pane ids when the pane set is discovered, never as a
  status source.
- No CLI equivalent exists (`herdr agent wait` is one-shot, one agent).
  The stream is socket-only. The CLI composition provides no stream
  (C1); no error variant is needed and none is added.

## Dispatch and PR topology

Stack child of AY.8: create the branch from
`feature/ay8-herdr-socket-transport` with `sc-git-worktree` once AY.8's
`SocketIo` and fake server are pushed (contracts first, same rule as the
core stack); do not wait for AY.8 to merge. PR target is AY.8's branch;
link with `gh stack link --base integrate/phase-ay
feature/ay8-herdr-socket-transport feature/ay10-herdr-status-stream`. If
the AY.8a/AY.8b split is exercised, the parent is AY.8b. Never merge an
unmerged parallel sibling into this branch.

## Deliverables

- [ ] D1 — `pub trait HerdrStatusStream` and the C1 event types in
  `crates/atm-herdr/src/lib.rs`. `HerdrProcessAdapter` is byte-identical
  to its AY.2 pin. `HerdrError` is unchanged. The public-item pin in
  `crates/atm-architecture/tests/boundary_enforcement.rs` gains exactly the
  C1 items; that is an additive minor change under ADR-061 and is recorded
  in the same commit.
- [ ] D2 — `crates/atm-herdr/src/status_stream.rs`: socket implementation
  of C1 over AY.8's endpoint resolver and NDJSON framing. One held
  connection per session, outside AY.8's 16 request permits and counted by
  its own permit of exactly 1 per session per invoker. C2 is the
  connection contract.
- [ ] D3 — composition: the transport factory in
  `crates/atm-herdr/src/transport.rs` returns, next to the
  `HerdrProcessInvoker`, an `Option<Arc<dyn HerdrStatusStream>>` that is
  `Some` for the socket transport and `None` for the CLI transport. The
  CLI transport has no `subscribe`; nothing is spawned; no CLI code
  changes.
- [ ] D4 — fake socket server (`tests/support/fake_herdr_socket/**`, AY.8
  D7) gains a streaming mode: accepts `events.subscribe`, records the
  subscription set, replies `SubscriptionStarted`, emits the per-pane
  probe event for each status-filtered subscription matching the scripted
  pane state, then emits scripted event lines, delays, EOF, and over-cap
  lines from a fixture script; it also serves scripted `agent.list`
  responses on one-request connections and counts open connections.
  Recordings live under `tests/fixtures/herdr-versions/0.8.2/events/`.
- [ ] D5 — tests in `crates/atm-herdr/tests/status_stream.rs` (C3 list)
  with paused Tokio time and no wall-clock sleeps.
- [ ] D6 — `docs/atm-herdr/herdr-versions.md`: `events.subscribe` column
  (absent 0.8.0; present 0.8.2 fbd20ad6; no replay 20a500a7; per-pane
  probe-on-subscribe semantics).
- [ ] D7 — `boundaries/atm-herdr/herdr-process-adapter.toml`: the AY.8
  `herdr_local_socket_client` key also owns `status_stream.rs`; no other
  boundary field changes. Composed-file rule P-E(b) applies (this sprint
  is the third editor; see phase-ay-plan.md P-E).
- [ ] D8 — `docs/atm-herdr/requirements.md` gains one requirement id,
  HR-CORE-011 (status stream: socket-only, one held connection per
  session, per-pane probe baseline, own reconnect budget, no nudge-breaker
  coupling), and ADR-058 gains an amendment paragraph recording that the
  subscription socket is added alongside the poll and that dropping the
  poll is a separate decision (item 7 of the rework record). No other
  requirement changes.

### Paths to delete

None.

## Code contracts

### C1 — trait

```rust
pub trait HerdrStatusStream: Send + Sync {
    /// Opens the held subscription for `session`. Returns after the server
    /// has acknowledged the subscription (`SubscriptionStarted`) and the
    /// per-pane probe events have been read for every discovered pane;
    /// the first event the receiver yields is `Baseline`.
    fn subscribe<'a>(
        &'a self,
        session: Option<&'a HerdrSession>,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrStatusEvents, HerdrError>> + Send + 'a>>;
}

/// Ordered, bounded receiver. Dropping it closes the connection and joins
/// the reader task; nothing outlives the receiver.
pub struct HerdrStatusEvents { /* crate-private fields */ }

impl HerdrStatusEvents {
    pub async fn next(&mut self) -> Option<HerdrStatusEvent>;
}

pub enum HerdrStatusEvent {
    /// Emitted once after every successful (re)subscribe: one entry per
    /// discovered pane, built from the per-pane probe events delivered on
    /// this connection.
    Baseline(Vec<AgentSnapshot>),
    Changed { agent: AgentName, status: HerdrAgentStatus, observed_at: Instant },
    Gone { agent: AgentName, observed_at: Instant },
    /// Connection lost; the implementation is reconnecting. The next event
    /// is `Baseline` or `Closed`.
    Disconnected { reason: HerdrError },
    /// Reconnect budget exhausted or deadline passed; receiver is finished.
    Closed { reason: HerdrError },
}
```

`AgentSnapshot`, `AgentName`, `HerdrAgentStatus`, `RequestDeadline`,
`HerdrSession`, and `HerdrError` are the existing AY.2 types; the enum
`HerdrError` does not change (HR-CORE-008 stays closed). No message
bodies, pane titles, or command lines are carried; the event maps pane
events to agent names using the same pane-to-agent projection `agent.list`
already uses.

### C2 — connection contract

```text
acquire the single stream permit for (invoker, session); every request
  below runs under that permit, never under AY.8's 16 request permits
discover: one agent.list on a one-request connection -> {agent -> pane_id}
connect (AY.8 endpoint resolution, deadline, pipe-busy retry)
send events.subscribe with:
  pane.created, pane.closed, pane.exited, pane.agent_detected (no pane id)
  PaneAgentStatusChanged { pane_id, agent_status: Some(s) } for every
    discovered pane and every s in {Idle, Working, Blocked, Done, Unknown}
read SubscriptionStarted; read the probe events (one per discovered pane
  whose status matched a filter); emit Baseline built from them
then read lines until EOF or error; map PaneAgentStatusChanged -> Changed,
  pane.closed / pane.exited -> Gone
on pane.created or pane.agent_detected for a pane not in the set:
  coalesce for 1 s, then open a replacement connection, subscribe it
  (discovery + the same subscription set + the new panes), wait for its
  SubscriptionStarted and probe events, emit Baseline from the
  replacement, and only then close the previous connection. The previous
  connection stays open until the replacement has started so no
  pane.created can fall between the two.
on EOF/error (including a line over the 1 MiB cap): emit Disconnected,
  back off (100 ms, 200, 400, doubling, capped at 5 s, at most 10
  consecutive failed attempts; atm's own choice, not a Herdr value),
  reconnect, resubscribe, emit Baseline; a successful resubscribe resets
  the attempt count
after the 10th consecutive failure or when the deadline passes: emit
  Closed and release the permit
```

Two connections exist for a session only during a replacement handover;
otherwise exactly one. Dropping the receiver cancels the reader task,
closes any connection, and releases the permit. No detached task. Runtime
drain rules are AY.8 C2's.

The reconnect budget is the stream's own. Stream failures never call into
the ADR-058 D10.1 nudge breaker (HR-SAFE-005..007), never open it, and
never read its state; the poll path's breaker is untouched by this sprint.
Whether stream failures should feed a breaker is decided in AY.12, when
the stream becomes a state source, not here.

### C3 — changed-file allowlist

- `crates/atm-herdr/src/lib.rs` (trait, event types)
- `crates/atm-herdr/src/status_stream.rs` (new)
- `crates/atm-herdr/src/transport.rs` (D3 factory return only)
- `crates/atm-herdr/src/transport_socket.rs` (expose framing helpers crate-private)
- `crates/atm-herdr/tests/status_stream.rs` (new)
- `crates/atm-herdr/tests/support/fake_herdr_socket/**`
- `crates/atm-herdr/tests/fixtures/herdr-versions/0.8.2/events/**` (new)
- `crates/atm-architecture/tests/boundary_enforcement.rs` (public-item pin: C1 items)
- `boundaries/atm-herdr/herdr-process-adapter.toml`
- `docs/atm-herdr/herdr-versions.md`
- `docs/atm-herdr/requirements.md` (HR-CORE-011)
- `docs/adr/` ADR-058 file (amendment paragraph)

No file under `crates/atm-daemon-bootstrap`, `crates/atm-http-runtime`,
or `crates/atm-daemon` changes. `transport_cli.rs` does not change.

### Size

One crate, one new module, one fixture mode, one test file, two doc
edits: expected to fit one context window. No split is pre-declared; if it
does not fit, that is a plan amendment, not a dispatch-time decision.

## Required work

1. Land D1/D3/D7/D8 (trait, factory return, pin, boundary, requirement
   id, ADR amendment) as the first commit.
2. Implement C2 against the fake server's streaming mode; close every
   handover, reconnect, resubscribe, cap, deadline, and cancellation case.
3. Prove nothing else changed: `HerdrProcessAdapter` pin unchanged,
   `HerdrError` unchanged, no daemon crate in the diff, C3 subset check.

## Acceptance criteria

1. `subscribe` on the socket transport yields `Baseline` as the first
   event and the factory returns `Some` for socket and `None` for CLI; no
   process is spawned on the CLI path.
2. Scripted fixtures pass for: status change delivery order; a status
   transition scripted to occur between the fake server's `agent.list`
   reply and `SubscriptionStarted` is delivered as the probe value in
   `Baseline` (nothing lost, nothing duplicated); pane.created triggers a
   replacement connection whose `Baseline` includes the new pane, with a
   pane.created scripted during the handover window still delivered; the
   old connection closes only after the replacement's
   `SubscriptionStarted`; EOF triggers `Disconnected` then `Baseline`
   within the backoff schedule under paused time; the 10th consecutive
   failure yields `Closed`; an over-cap line yields `Disconnected`;
   dropping the receiver joins the task and releases the permit (no
   detached task, asserted with the AY.8 cancellation harness).
3. Exactly one held connection per session is observable at the fake
   server while the receiver lives (two only during a handover),
   regardless of agent count (fixture with 50 panes); a churn storm of 20
   pane.created events within 1 s yields one replacement connection, and
   AY.8's 16 request permits are never acquired by the stream (fake
   invoker asserts the permit count stays at 16 throughout).
4. Stream failures leave the nudge breaker state unchanged (breaker
   observed closed before and after 10 scripted stream failures).
5. `git diff <parent>..HEAD -- crates/atm-daemon-bootstrap
   crates/atm-http-runtime crates/atm-daemon crates/atm-herdr/src/transport_cli.rs`
   is empty; the PR file set is a subset of C3.
6. `HerdrProcessAdapter` byte-identical to the AY.2 pin; `HerdrError`
   byte-identical; the public-item pin diff is exactly the C1 items.
7. HR-CORE-011 present in `docs/atm-herdr/requirements.md` and the
   ADR-058 amendment paragraph present.
8. Common phase merge gate (zero blocking/important/in-scope minor,
   quality-mgr PASS, three CI lanes green at merge).

## Required validation

- `just validate` on all three CI lanes.
- `cargo test -p atm-herdr -p atm-architecture`.
- `python3 .just/check_line_counts.py`.
- Compare the PR file list mechanically against C3.

## Out of scope

- Consuming the stream anywhere (AY.11).
- Changing the queue-wake tick or poll interval (AY.11, AY.12).
- Coupling stream failures to the nudge breaker (decided in AY.12).
- Any Herdr change; any CLI subscribe emulation; any `HerdrError` change.
- The legacy synchronous daemon.
