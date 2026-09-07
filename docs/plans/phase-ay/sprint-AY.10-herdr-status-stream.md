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
    rationale: AY.9 owns the daemon-bootstrap composition and doctor files; AY.10 touches only atm-herdr, its fixtures, the architecture pin, and the Herdr requirement docs, and is consumed by nothing until AY.11. The shared `lib.rs`, `transport.rs`, and `crates/atm-architecture/tests/boundary_enforcement.rs` edits are composed under the P-E rule in phase-ay-plan.md (keep both sides' items and pinned entries).
  - prerequisite: AY.10
    dependent: AY.11
    relation: must_follow
    rationale: AY.11 consumes the stream trait this sprint lands.
---

# AY.10 — Herdr agent-status subscription stream in atm-herdr, behind a trait

Add one held Herdr `events.subscribe` connection per configured session that
delivers agent-status changes to atm as Herdr observes them, exposed through
a new trait in `atm-herdr`. Nothing consumes it in this sprint. The daemon's
polling behaviour is unchanged. What the held connection costs inside Herdr
is stated in "Herdr facts" and is Rand's accepted-cost decision under
phase-ay-plan.md rework item 7, recorded before this sprint is dispatched.

## Why (Rand, 2026-09-06)

"we shouldn't be creating 50 sockets every 5 seconds"; "I would expect all
agent queries would be done on a single socket."; "yes, I think we need to
add the subscription socket."; "are you proposing we do both and verify
they match. once they match we can drop the polling?"; "this is a prudent
solution."; "and probably lowest risk". This sprint is the "add the
subscription socket" half. AY.11 is the "do both and verify they match"
half. Dropping the poll is AY.12, gated on AY.11's evidence and Rand's
decision.

## Herdr facts this sprint rests on (v0.8.2 9eb52145, `src/api`, `src/app`)

- `events.subscribe` (`schema.rs:231`, `schema/events.rs:12`) takes
  `{"subscriptions":[...]}`, replies `SubscriptionStarted`, then streams one
  JSON line per matching event until the client closes
  (`server.rs` `stream_subscriptions`, ~735-751). It is the only long-lived
  request shape atm uses; every other request stays
  one-request-per-connection (AY.8 C2).
- Herdr's stream is server-side polled, not pushed. `stream_subscriptions`
  polls every subscription on the connection every 100 ms
  (`CONNECTION_POLL_INTERVAL`, `server.rs:28`) and returns at most one event
  per subscription per tick.
- `Subscription::PaneAgentStatusChanged { pane_id, agent_status:
  Option<AgentStatus> }` (`schema/events.rs:76`; `agent_status` omitted =
  every change) is per pane. At subscribe Herdr records the event-hub
  sequence, then runs one `pane_get` probe for the pane (`subscriptions.rs`
  ~255-285). On each tick (`poll_result`, ~389-470) it drains hub
  `PaneAgentStatusChanged` events for the pane; when the hub has had no new
  event at all since the previous tick it runs one more `pane_get` and emits
  a change if the pane's status or presentation differs from its last
  snapshot. Consequences this sprint is designed around:
  1. Cost: every `PaneAgentStatusChanged` subscription costs Herdr one
     in-process `pane_get` (a channel round trip to Herdr's app thread, not
     a socket) per 100 ms while the hub is quiet. With one subscription per
     pane that is 10N `pane_get`/s inside Herdr for N subscribed panes
     (500/s at the plan's 50-agent target). A status filter does not reduce
     it, so one unfiltered subscription per pane is the minimum for this
     subscription kind; the round-3 design of five filtered subscriptions
     per pane (5N, 2,500/s at 50 panes) is withdrawn.
  2. No zero-poll form exists at 0.8.2. `PaneAgentStatusChanged` is emitted
     to the hub on every status or presentation change (`src/app/api.rs`
     ~640-660) but is subscribable only per pane through the probing
     subscription above. `pane.updated` is hub-only but is emitted on
     agent-name, title, and runtime changes (`api.rs:618`,
     `runtime.rs:338,444`, `terminal_titles.rs:73`, `agents.rs:60,136`), not
     on status changes, so it cannot carry status.
  3. Duplicates are normal: a change after the sequence capture is on the
     hub and is delivered even when the subscribe probe already reflected
     it. The client dedupes by value (C2).
- Hub-only subscriptions (`pane.created`, `pane.closed`, `pane.exited`,
  `pane.agent_detected`; no pane id) drain the event hub only, with no
  `pane_get`, and are created with `last_sequence: 0` (`subscriptions.rs`
  ~180-215), so every (re)subscribe replays whatever the hub still retains
  (512-entry ring, `event_hub.rs`; only structural events are pushed to the
  hub, pane output is not). C2 therefore treats these events as re-list
  triggers, never as facts, and their one-event-per-tick drain rate is
  irrelevant because the trigger is coalesced.
- The wire `EventEnvelope { event, data }` (`schema/events.rs:362`)
  carries no sequence number and no probe-versus-live marker. The baseline
  is therefore taken from `agent.list` after `SubscriptionStarted` (C2),
  not from the stream's first events.
- `agent.list` returns `pane_id` and `agent_status` per agent; it is used
  here to discover the pane set and as the baseline snapshot.
- No CLI equivalent exists (`herdr agent wait` is one-shot, one agent).
  The stream is socket-only. The CLI composition provides no stream
  (C1); no error variant is needed and none is added.
- Bound this sprint adds inside Herdr while a stream is held: one
  `pane_get` per subscribed pane per 100 ms while the hub is quiet, plus N
  sequential `pane_get` probes per (re)subscribe handshake. Zero new
  sockets; the poll path's one `agent.list` per 5 s is unchanged until
  AY.12. AY.11's dogfood evidence records Herdr's process CPU with and
  without the stream open so the cost is measured, not only derived.

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
  D7) gains a streaming mode that models the Herdr facts above, not an
  idealised push server: accepts `events.subscribe`, records the
  subscription set, runs one scripted-latency probe per
  `PaneAgentStatusChanged` subscription in order and replies
  `SubscriptionStarted` only after the last probe, replays scripted
  retained hub history (e.g. old `pane.created` lines) on every subscribe,
  then emits scripted event lines (including lines whose status equals the
  pane's current value), delays, EOF, and over-cap lines from a fixture
  script; serves scripted `agent.list` responses with scripted reply
  latency on one-request connections; counts open connections and the
  subscription entries per connection. Recordings live under
  `tests/fixtures/herdr-versions/0.8.2/events/`.
- [ ] D5 — tests in `crates/atm-herdr/tests/status_stream.rs` (C3 list)
  with paused Tokio time and no wall-clock sleeps.
- [ ] D6 — `docs/atm-herdr/herdr-versions.md`: `events.subscribe` column
  (absent 0.8.0; present 0.8.2 fbd20ad6; agent-status subscriptions start
  at the live sequence, 20a500a7, and fall back to one `pane_get` per pane
  per 100 ms tick; hub-only subscriptions replay retained history).
- [ ] D7 — `boundaries/atm-herdr/herdr-process-adapter.toml`: the AY.8
  `herdr_local_socket_client` key also owns `status_stream.rs`; no other
  boundary field changes. Composed-file rule P-E(b) applies (this sprint
  is the third editor; see phase-ay-plan.md P-E).
- [ ] D8 — `docs/atm-herdr/requirements.md` gains one requirement id,
  HR-CORE-011 (status stream: socket-only, one held connection per
  session, one unfiltered `PaneAgentStatusChanged` subscription per pane,
  `agent.list` baseline after `SubscriptionStarted`, value-deduped
  changes, own reconnect budget, no nudge-breaker coupling, Herdr-side
  cost bound as stated in this sprint), and ADR-058 gains an amendment paragraph recording that the
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
    /// `agent.list` baseline snapshot has been taken; the first event the
    /// receiver yields is `Baseline`. The handshake runs under the C2
    /// handshake budget, never longer than `deadline`.
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
    /// Emitted once after every successful (re)subscribe or replacement:
    /// one entry per listed agent, from the `agent.list` snapshot taken
    /// after `SubscriptionStarted`, overridden per agent by any status
    /// event read from the stream before the snapshot reply.
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
discover: one agent.list on a one-request connection -> pane set
  {agent -> pane_id}
connect (AY.8 endpoint resolution, pipe-busy retry)
send events.subscribe with:
  pane.created, pane.closed, pane.exited, pane.agent_detected (no pane id)
  pane.agent_status_changed { pane_id } (no agent_status filter) for every
    pane in the pane set
read SubscriptionStarted
snapshot: one agent.list on a one-request connection; emit Baseline from
  it, except that for any agent whose pane.agent_status_changed was read
  from the stream between SubscriptionStarted and the snapshot reply the
  stream value wins (it is newer); record every agent's held value
then read lines until EOF or error:
  pane.agent_status_changed whose status differs from the held value for
    that agent -> Changed, update the held value; equal -> dropped (Herdr
    delivers duplicates, see Herdr facts); unknown pane -> re-list trigger
  pane.created / pane.agent_detected / pane.closed / pane.exited -> re-list
    trigger only (hub-only subscriptions replay retained history on every
    subscribe, so these lines are never treated as facts)
re-list trigger: coalesce for 1 s, then one agent.list on a one-request
  connection; pane set unchanged -> nothing; agents no longer listed ->
  Gone; new panes -> replacement connection: run this procedure again with
  the new pane set, wait for the replacement's Baseline to be emitted, and
  only then close the previous connection (nothing falls between the two)
handshake budget: connect + subscribe + snapshot must complete within
  min(deadline, 2 s + 20 ms x panes in the set) (atm's choice: Herdr runs
  one sequential pane_get probe per subscribed pane before
  SubscriptionStarted; 3 s at 50 panes); a handshake past its budget is a
  failed attempt below
on EOF/error (including a line over the 1 MiB cap): emit Disconnected,
  back off (100 ms, 200, 400, doubling, capped at 5 s, at most 10
  consecutive failed attempts; atm's own choice, not a Herdr value),
  reconnect, resubscribe, emit Baseline; a successful resubscribe resets
  the attempt count
after the 10th consecutive failure or when the deadline passes: emit
  Closed and release the permit
```

Why this loses nothing: Herdr records the hub sequence before probing each
pane, so every status change after subscribe reaches the stream; the
snapshot is taken after `SubscriptionStarted`, so a change before the
snapshot is in the snapshot, a change after it is on the stream, and a
change seen by both is the same value twice and is dropped by the held
value comparison. The stream never needs a probe-versus-live marker.

Two connections exist for a session only during a replacement handover;
otherwise exactly one. The subscription set on a connection is exactly
4 + N entries for N panes. Dropping the receiver cancels the reader task,
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
2. Implement C2 against the fake server's streaming mode (sequential
   scripted-latency probes, replayed history, duplicate-valued events);
   close every handover, reconnect, resubscribe, cap, handshake-budget,
   deadline, and cancellation case.
3. Prove nothing else changed: `HerdrProcessAdapter` pin unchanged,
   `HerdrError` unchanged, no daemon crate in the diff, C3 subset check.

## Acceptance criteria

1. `subscribe` on the socket transport yields `Baseline` as the first
   event and the factory returns `Some` for socket and `None` for CLI; no
   process is spawned on the CLI path.
2. Scripted fixtures pass for: status change delivery order; a status
   transition scripted between two panes' subscribe probes is delivered
   exactly once (in `Baseline` or as one `Changed`, never both); a
   transition scripted between `SubscriptionStarted` and the `agent.list`
   reply appears in `Baseline` with the stream value and yields no
   `Changed`; a stream line whose status equals the held value yields
   nothing; replayed `pane.created` history on subscribe with an unchanged
   pane set yields one `agent.list` and no replacement (connection count
   stays 1); `pane.created` for a new pane yields a replacement whose
   `Baseline` includes it, with a `pane.created` scripted during the
   handover still acted on; the old connection closes only after the
   replacement's `Baseline`; a listed agent that disappears yields `Gone`;
   EOF yields `Disconnected` then `Baseline` within the backoff schedule
   under paused time; the 10th consecutive failure yields `Closed`; an
   over-cap line yields `Disconnected`; dropping the receiver joins the
   task and releases the permit (no detached task, asserted with the AY.8
   cancellation harness).
3. Exactly one held connection per session is observable at the fake
   server while the receiver lives (two only during a handover),
   regardless of agent count (fixture with 50 panes); the fake server
   asserts the subscription set is exactly 4 + N entries with no
   `agent_status` filter; a churn storm of 20 pane.created events within
   1 s yields one `agent.list` and one replacement connection; AY.8's 16
   request permits are never acquired by the stream (fake invoker asserts
   the permit count stays at 16 throughout).
3a. Handshake budget: with 50 panes and 10 ms scripted latency per probe
   plus 50 ms `agent.list` latency, `subscribe` completes inside the 3 s
   budget under paused time; with 100 ms per probe the handshake is
   reported as a failed attempt (`Disconnected`, then backoff) and the
   budget is `min(deadline, 2 s + 20 ms x N)` by assertion at N = 1 and
   N = 50.
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
- Any Herdr change, including a hub-only agent-status subscription that
  would remove the per-pane `pane_get` cost; if Rand chooses that route
  under rework item 7, this sprint is re-planned, not stretched. Any CLI
  subscribe emulation; any `HerdrError` change.
- The legacy synchronous daemon.
