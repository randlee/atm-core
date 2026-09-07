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
    rationale: AY.9 owns the daemon-bootstrap composition and doctor files; AY.10 touches only atm-herdr and its fixtures and is consumed by nothing until AY.11.
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
- Subscriptions are per pane: `PaneAgentStatusChanged { pane_id,
  agent_status: Option<..> }` (`schema/events.rs:76`). `pane.created`,
  `pane.closed`, `pane.agent_detected`, `pane.exited` are subscribable
  without a pane id. Adding a pane to the set requires a new
  `events.subscribe` request, which is a new connection.
- The stream starts at the live sequence with no replay (drift table,
  20a500a7). Events missed while disconnected are gone; the only way to
  recover state is one `agent.list`.
- No CLI equivalent exists (`herdr agent wait` is one-shot, one agent).
  The stream is socket-only; the CLI transport reports it unsupported.

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

- [ ] D1 — `pub trait HerdrStatusStream` in `crates/atm-herdr/src/lib.rs`
  (C1). `HerdrProcessAdapter` is byte-identical to its AY.2 pin. The
  public-item pin gains exactly the C1 items; that is an additive minor
  change under ADR-061 and is recorded in the pin test in the same commit.
- [ ] D2 — `crates/atm-herdr/src/status_stream.rs`: socket implementation
  of C1 over AY.8's endpoint resolver and NDJSON framing. One held
  connection per `subscribe` call, outside AY.8's 16 request permits and
  counted by its own permit of exactly 1 per session per invoker. C2 is
  the connection contract.
- [ ] D3 — CLI transport: `subscribe` returns
  `HerdrError::Unsupported { feature: "events.subscribe" }` immediately;
  no process is spawned. `HerdrError` gains that one variant (HR-CORE-008
  stays closed; variant added in the same commit as the pin update).
- [ ] D4 — fake socket server (`tests/support/fake_herdr_socket/**`, AY.8
  D7) gains a streaming mode: accepts `events.subscribe`, replies
  `SubscriptionStarted`, then emits scripted event lines, delays, EOF, and
  over-cap lines from a fixture script. Recordings live under
  `tests/fixtures/herdr-versions/0.8.2/events/`.
- [ ] D5 — tests in `crates/atm-herdr/tests/status_stream.rs` (C3 list)
  with paused Tokio time and no wall-clock sleeps.
- [ ] D6 — `docs/atm-herdr/herdr-versions.md`: `events.subscribe` column
  (absent 0.8.0; present 0.8.2 fbd20ad6; no replay 20a500a7).
- [ ] D7 — `boundaries/atm-herdr/herdr-process-adapter.toml`: the AY.8
  `herdr_local_socket_client` key also owns `status_stream.rs`; no other
  boundary field changes. Composed-file rule P-E(b) applies.

### Paths to delete

None.

## Code contracts

### C1 — trait

```rust
pub trait HerdrStatusStream: Send + Sync {
    /// Opens one held subscription for `session`. Returns after the server
    /// acknowledges the subscription and the baseline `agent.list` has been
    /// taken; never returns a stream that has not emitted `Baseline`.
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
    /// Emitted once after every successful (re)subscribe: the full
    /// `agent.list` snapshot taken on that connection.
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
`HerdrSession`, and `HerdrError` are the existing AY.2 types. No message
bodies, pane titles, or command lines are carried; the event maps pane
events to agent names using the same pane-to-agent projection `agent.list`
already uses.

### C2 — connection contract

```text
acquire the single stream permit for (invoker, session)
connect (AY.8 endpoint resolution, deadline, pipe-busy retry)
send one agent.list on a separate one-request connection; emit Baseline
send events.subscribe with: pane.created, pane.closed, pane.agent_detected,
  pane.exited, and PaneAgentStatusChanged for every pane in the baseline
read SubscriptionStarted; then read lines until EOF or error
on pane.created or pane.agent_detected for an unknown pane:
  close, reconnect, resubscribe (which re-takes the baseline)
on EOF/error: emit Disconnected, back off (100 ms, 200, 400, capped 5 s,
  max 10 attempts), reconnect, resubscribe
line cap 1 MiB; a longer line is a protocol error -> Disconnected
```

The receiver owns the reader task; `Drop` cancels it and releases the
permit. No detached task. Runtime drain rules are AY.8 C2's. Reconnect
attempts are infrastructure-class failures under the existing breaker
policy (HR-SAFE-005..007); the breaker opening ends the stream with
`Closed`.

### C3 — changed-file allowlist

- `crates/atm-herdr/src/lib.rs` (trait, event types, one error variant, pin)
- `crates/atm-herdr/src/status_stream.rs` (new)
- `crates/atm-herdr/src/transport_socket.rs` (expose framing helpers crate-private)
- `crates/atm-herdr/src/transport_cli.rs` (D3 only)
- `crates/atm-herdr/tests/status_stream.rs` (new)
- `crates/atm-herdr/tests/support/fake_herdr_socket/**`
- `crates/atm-herdr/tests/fixtures/herdr-versions/0.8.2/events/**` (new)
- `crates/atm-herdr/tests/public_item_pin.rs` (or the AY.2 pin's actual file)
- `boundaries/atm-herdr/herdr-process-adapter.toml`
- `docs/atm-herdr/herdr-versions.md`

No file under `crates/atm-daemon-bootstrap`, `crates/atm-http-runtime`,
or `crates/atm-daemon` changes.

### Size

One crate, one new module, one fixture mode, one test file: expected to
fit one context window. No split is pre-declared; if it does not fit, that
is a plan amendment, not a dispatch-time decision.

## Required work

1. Land D1/D3/D7 (trait, error variant, pin, boundary) as the first commit.
2. Implement C2 against the fake server's streaming mode; close every
   reconnect, resubscribe, cap, deadline, and cancellation case.
3. Prove nothing else changed: `HerdrProcessAdapter` pin unchanged, no
   daemon crate in the diff, C3 subset check.

## Acceptance criteria

1. `subscribe` on the socket transport emits `Baseline` before returning
   the receiver; on the CLI transport returns `Unsupported` without
   spawning.
2. Scripted fixtures pass for: status change delivery order; pane.created
   triggers resubscribe and a fresh `Baseline`; EOF triggers
   `Disconnected` then `Baseline` within the backoff schedule under paused
   time; 11th failure yields `Closed`; over-cap line yields
   `Disconnected`; dropping the receiver joins the task and releases the
   permit (no detached task, asserted with the AY.8 cancellation harness).
3. Exactly one held connection per session is observable at the fake
   server while the receiver lives, regardless of agent count (fixture
   with 50 panes).
4. `git diff <parent>..HEAD -- crates/atm-daemon-bootstrap
   crates/atm-http-runtime crates/atm-daemon` is empty; the PR file set is
   a subset of C3.
5. `HerdrProcessAdapter` byte-identical to the AY.2 pin; the public-item
   pin diff is exactly the C1 items and the one error variant.
6. Common phase merge gate (zero blocking/important/in-scope minor,
   quality-mgr PASS, three CI lanes green at merge).

## Required validation

- `just validate` on all three CI lanes.
- `cargo test -p atm-herdr`.
- `python3 .just/check_line_counts.py`.
- Compare the PR file list mechanically against C3.

## Out of scope

- Consuming the stream anywhere (AY.11).
- Changing the queue-wake tick or poll interval (AY.11, AY.12).
- Any Herdr change; any CLI subscribe emulation.
- The legacy synchronous daemon.
