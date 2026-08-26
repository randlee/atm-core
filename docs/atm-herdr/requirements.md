# ATM-Herdr Crate Requirements

## 1. Purpose

This document defines the `atm-herdr` crate requirements.

The `atm-herdr` crate owns the Tokio-native process boundary to the external
`herdr` CLI: argv construction, child-process spawning and bounding,
stdout/stderr parsing, typed result and error mapping, and the per-host
spawn circuit breaker. It is the sole place in the workspace that knows
`herdr` argv shapes, Herdr's structured JSON error vocabulary, or Herdr's
exit-code contract.

Product behavior remains defined in
[`../requirements.md`](../requirements.md). `atm-herdr` satisfies the
Phase AQ Herdr local-steer-backend product requirements without re-owning
`atm-core` roster/delivery-classification semantics, `atm-daemon-bootstrap`
emitter/selector wiring, or `atm-http-runtime` queue-pump orchestration.

This crate is introduced by Phase AQ (sprints AQ2.6 and AQ2.7) and is
governed by [ADR-058](../adr/ADR-058-herdr-local-steer-backend-contract.md),
which pins the Herdr release and records every argv, exit-code, and
error-code claim this crate relies on. Where any other document disagrees
with ADR-058 on Herdr's own behavior, ADR-058 is authoritative; this
document cites it by decision id (`D1`-`D10.1`).

## 2. Ownership

`atm-herdr` owns:

- the `HerdrProcessAdapter` trait: the async `prompt` / `wait` / `get` /
  `list` contract consumed by the immediate steer path and the AQ2.7
  queue-tick pump
- `HerdrProcessInvoker`, the concrete `tokio::process`-backed
  implementation: argv construction for every `herdr agent ...` shape this
  crate emits (`prompt`, `wait`, `get`, `list`), `HERDR_SESSION` set on the
  **child process environment, per invocation**, only when the calling
  member's roster row (or, for `list`, the caller-supplied session)
  carries a `Some` session (ADR-058 D1's per-member model; the daemon's
  own process environment is never consulted), one `list` child per
  distinct session, and a flat 5 s external kill bound wrapped around
  every spawned child independent of any `--timeout` argv value
  (ADR-058 D10)
- typed results: `AgentSnapshot { name, status, workspace_id }`, the
  `prompt` / `wait` / `get` outcome types built from it, and `list`'s
  `Vec<AgentSnapshot>` result
- `HerdrError`, the enum mapping Herdr's structured stderr JSON
  `error.code` values (`agent_blocked`, `agent_not_found`,
  `agent_not_ready`, `agent_target_ambiguous`, `agent_not_running`,
  `agent_prompt_stalled`, `server_not_running`, `protocol_mismatch`,
  `timeout`, `invalid_agent_name`, `empty_agent_prompt`) and Herdr's exit
  codes (0 / 1 / 2 / other) to a closed, typed Rust vocabulary
- `HerdrSpawnBreaker`: a per-host circuit breaker type, exponential
  `1 s * 2^n` backoff capped at `30 s`, half-open single-probe recovery,
  opened by the infrastructure-class outcomes `server_not_running`,
  `protocol_mismatch`, an external-timeout kill, a failed `agent list`
  call, or a failed `agent get` doctor/presence probe (ADR-058 D10.1,
  D9)
- a fake `HerdrProcessAdapter` implementation behind a `test-utils` Cargo
  feature, recording every call for assertion, for use by every consumer
  crate's tests
- the Herdr version/protocol pin: `herdr` `0.8.2`, wire protocol `20`
  (ADR-058 "Pinned Herdr revision"), and the fixture-currency obligation
  that keeps this crate's parsing in step with a pin bump

`atm-herdr` does not own:

- `LocalMessageReceivedBackend::Herdr { session: Option<HerdrSession> }`
  and `HerdrSession` — roster-derived routing data owned by `atm-core`
  (AQ1, `crates/atm-core/src/delivery_channel.rs`); `atm-herdr` consumes an
  already-resolved `AgentName` and an already-validated optional session
  string, it does not derive either from roster metadata
- `HerdrReceivedHook`, the immediate-steer emitter that implements the
  sealed `AsyncMessageReceivedHookEmitter` contract and decides *when* to
  call this crate's `prompt` — owned by `atm-daemon-bootstrap` (AQ2.6)
- the queue-tick pump: its 5 s polling cadence, per-session `agent list`
  grouping, per-tick prompt cap, FIFO claim ordering, and `RuntimeHealth`
  recording — owned by `atm-http-runtime` (AQ2.7, `HerdrQueueWakePump`);
  `atm-herdr` supplies only `list` / `prompt` / `get` and the breaker
- queue state: `PendingNudgeStore`, `MemberKey`, `NudgeClaim`,
  `claim_next_pending` / `requeue_pending` / `release_pending` /
  `list_pending_members` — owned by `atm-storage` (AQ1)
- `atm doctor` presence-probe reporting, roster-consistency findings, or
  CLI/`teams add-member`/`update-member` surface — owned by `atm-core` and
  the `atm` CLI, which call this crate's adapter but interpret and render
  the result themselves
- launching, starting, or renaming a Herdr agent session (`agent start`,
  `agent rename`) — these are operator/launch-convention commands
  (ADR-058 D6) that atm-core never runs; this crate defines no argv for
  them

## 3. Requirement Namespace

The `atm-herdr` crate uses the `HR-*` namespace, grouped by category:

- `HR-CORE-*` — functional requirements
- `HR-SAFE-*` — safety requirements
- `HR-OBS-*` — observability requirements
- `HR-TEST-*` — test requirements
- `HR-VER-*` — version and pin policy

### 3.1 Functional Requirements

- `HR-CORE-001` `atm-herdr` owns the `HerdrProcessAdapter` trait with
  `prompt`, `wait`, `get`, and `list` methods, each returning a typed
  outcome or `HerdrError` over an injected external deadline. This is the
  only cross-crate contract point; no consumer constructs `herdr` argv
  itself.
- `HR-CORE-002` `HerdrProcessInvoker::prompt` emits exactly
  `herdr agent prompt <AgentName> "You have unread ATM messages. Run: atm
  read"` (ADR-058 D2) with no `--wait` and no other flag. The prompt text is
  a crate-level constant; it is never interpolated with caller-supplied
  content, and the sender's original message body is never passed to
  Herdr.
- `HR-CORE-003` `HerdrProcessInvoker::wait` emits exactly
  `herdr agent wait <AgentName> --until idle --until done --until blocked
  --timeout <ms>` (ADR-058 D2) with an explicit, caller-supplied timeout on
  every call; an indefinite wait (omitted `--timeout`) is impossible by
  construction. `--until unknown` is never emitted. **Contracted, not
  invoked**: per Rand's 2026-08-26 decision, no Phase AQ caller invokes
  `wait` — the AQ2.7 queue pump uses `list` + `prompt` instead
  (`HR-CORE-005`, `architecture.md` §6). `wait` is retained on the trait
  for a possible future lifecycle-gated delivery mode and must remain
  argv/parsing-correct even though nothing calls it in Phase AQ.
- `HR-CORE-004` `HerdrProcessInvoker::get` emits exactly
  `herdr agent get <AgentName>` (ADR-058 D9) with no other argument. This
  is the doctor/presence-probe read of one named agent.
- `HR-CORE-005` `HerdrProcessInvoker::list` emits exactly
  `herdr agent list` with no other argument, one child per distinct
  session (`HERDR_SESSION` set on that child's environment only when the
  caller passes a `Some` session), and returns every agent Herdr reports
  as a `Vec<AgentSnapshot>`. This is the AQ2.7 queue pump's sole
  liveness/status discovery call (Rand's 2026-08-26 decision;
  `architecture.md` §6) — the pump never calls `wait` in Phase AQ. Fields
  read from Herdr's `AgentList` response are exactly `AgentInfo.name`,
  `AgentInfo.agent_status`, and `AgentInfo.workspace_id`
  (`src/api/schema/agents.rs:184-208` in the pinned Herdr checkout),
  returned under `ResponseResult::AgentList`
  (`src/app/api/agents.rs:19`); no other `AgentInfo` field is read.
- `HR-CORE-006` `HERDR_SESSION` is set on the spawned child's environment,
  per invocation, if and only if the caller passes a non-empty session
  string; when the caller passes none, the child inherits the ambient
  environment unmodified and Herdr resolves its default socket
  (`~/.config/herdr/herdr.sock`, ADR-058 D1). This crate never reads
  `HERDR_SESSION` or `HERDR_SOCKET_PATH` from its own process environment
  to synthesize a session choice; the caller's explicit input is the only
  source.
- `HR-CORE-007` `AgentSnapshot { name, status, workspace_id }` is the
  typed projection of Herdr's `AgentInfo` this crate exposes, shared by
  `prompt`, `wait`, `get`, and `list`. Only `agent_status` (mapped to a
  closed `HerdrAgentStatus` enum: `Idle`, `Working`, `Blocked`, `Done`,
  `Unknown`) is a contracted field consumers may branch on; `name` and
  `workspace_id` are informational passthrough only (ADR-058 "Explicitly
  NOT relied upon": other `AgentInfo` fields and JSON key order carry no
  contract).
- `HR-CORE-008` `HerdrError` is a closed enum with one variant per stderr
  `error.code` value this crate parses (`HR-SAFE-004` table) plus a
  transport/protocol variant family (`ServerNotRunning`, `ProtocolMismatch`,
  `ServerUnavailable`, `InternalError`) and a crate-owned `TimedOut`
  variant for this crate's own external deadline (`HR-SAFE-002`). An
  unrecognized `error.code` string maps to a generic advisory variant
  carrying the raw code; `atm-herdr` never matches on `error.message` text
  or JSON key order.
- `HR-CORE-009` `HerdrSpawnBreaker` is constructed once per host process
  and shared by every `HerdrProcessInvoker` call across every member
  (ADR-058 D10.1: "per-host circuit breaker shared by every Herdr member").
  `atm-herdr` defines the type; the composition root (`atm-daemon-bootstrap`)
  owns the single shared instance — see `architecture.md` §9.

### 3.2 Safety Requirements

- `HR-SAFE-001` `atm-herdr` never falls back to `agent send-keys`,
  `pane send-keys`, `pane send-input`, or any other raw-keystroke Herdr
  surface, and never shells out through `tmux send-keys` or an equivalent
  terminal-injection mechanism. `agent prompt`'s own `agent_blocked`
  pre-write rejection (ADR-058 D4) is the only accepted safety gate; a
  rejected prompt is a terminal outcome for this crate, not a trigger for
  an alternate delivery mechanism.
- `HR-SAFE-002` Every `herdr` child spawn is wrapped in an external,
  `atm-herdr`-owned, flat **5 s** deadline, independent of any `--timeout`
  argv value, because Herdr's own client applies no read/connect timeout
  (ADR-058 D10). This bound is identical for `prompt`, `get`, and `list`
  children — there is no per-command variance and no separate grace
  period. On elapse the child is killed and reaped (never merely
  killed-and-abandoned) before this crate returns `HerdrError::TimedOut`.
  (`wait` is contracted (`HR-CORE-003`) but not invoked by any Phase AQ
  caller, so its own external-bound question does not arise in this
  phase.)
- `HR-SAFE-003` The sender's original message body is never passed to
  Herdr on any code path. The only text this crate ever writes into a
  Herdr child's argv is the fixed mailbox-read prompt (`HR-CORE-002`);
  untrusted message content cannot become terminal input through this
  crate.
- `HR-SAFE-004` `atm-herdr` holds no durable or long-lived state about
  ATM mail, roster membership, or delivery/queue status. The crate's only
  runtime state is the per-host `HerdrSpawnBreaker`'s failure counter and
  open/half-open timer; it is process-lifetime, in-memory, and never
  persisted to SQLite or the roster. A restarted daemon starts with a
  closed breaker.
- `HR-SAFE-005` `HerdrSpawnBreaker` opens only on the infrastructure-class
  outcomes named in `HR-CORE-009`'s citation (`server_not_running`,
  `protocol_mismatch`, an external-timeout kill, a failed `agent list`
  call, or a failed `agent get` probe) and never on a lifecycle/target-shaped
  outcome (`agent_blocked`, `agent_not_found`, `agent_not_ready`,
  `agent_target_ambiguous`) — those are per-target conditions a breaker
  trip cannot help and must not suppress future attempts for unrelated
  members.
- `HR-SAFE-006` While the breaker is open, `prompt` / `wait` / `get` /
  `list` return `HerdrError::Unavailable { retry_after }` without spawning
  a child. This
  crate does not itself decide what a caller does with that outcome
  (dropped steer, released queue claim, or doctor warning are all
  caller-owned per `atm doctor` / `atm-http-runtime` / `atm-daemon-bootstrap`
  policy) — it only refuses to spawn and reports the remaining backoff.

### 3.3 Observability Requirements

- `HR-OBS-001` Every structured event this crate emits carries
  `subsystem="atm_herdr"` plus `action` and `outcome` fields, matching the
  workspace's structured-event convention (precedent:
  `subsystem="atm_core.queue"` in AQ1's `PendingNudgeStore` call sites).
- `HR-OBS-002` A child spawn, its argv shape (command name only —
  `prompt` / `wait` / `get` — never the literal mailbox-read text or an
  agent name beyond what is already roster-visible), its outcome
  (`accepted`, error variant, or `timed_out`), and its wall-clock duration
  are observable via a `tracing::debug!`/`tracing::warn!` event on every
  invocation.
- `HR-OBS-003` A breaker state transition (`closed -> open`,
  `open -> half_open probe`, `half_open -> closed`,
  `half_open -> open` on a failed probe) emits one `tracing::warn!` (open)
  or `tracing::info!` (recovery) event with `subsystem="atm_herdr"`,
  `action="spawn_breaker_transition"`, the new state, and
  `consecutive_failures`.
- `HR-OBS-004` A `HerdrError::TimedOut` outcome always emits a
  `tracing::warn!` event distinguishing which of the two external bounds
  (`HR-SAFE-002`) fired, so a hung Herdr server is distinguishable in logs
  from Herdr's own `timeout` error code without cross-referencing exit
  status.

### 3.4 Test Requirements

- `HR-TEST-001` A fake `HerdrProcessAdapter` implementation, gated behind
  the `test-utils` Cargo feature, records every `prompt` / `wait` / `get` /
  `list` call (agent, session, and — for `wait` — the requested `--until`
  set and timeout) for assertion and is configurable to return any
  `HerdrError` variant or outcome. It is the sole test double any
  consumer crate uses below the adapter boundary (precedent: AQ2.6/AQ2.7's
  `forbidden_test_bypasses` rule forbidding a real `HerdrProcessInvoker` in
  non-live test paths).
- `HR-TEST-002` Argv-construction tests assert byte-for-byte equality
  against `herdr-cli-contract-fixture.md`'s F1/F2/F3 argv rows for every
  emitted shape (`prompt`, `wait`, `get`, `list`), including the `--until`
  ordering and the millisecond `--timeout` value, so a future refactor
  cannot silently drift from the pinned contract.
- `HR-TEST-003` Stderr-parsing tests cover every row of ADR-058 D8's
  error-code table plus F1.8/F2.8/F3.5's argv-construction-bug rows
  (asserted unreachable by construction, never merely "not tested"), each
  reading only `error.code` and never `error.message` text or JSON key
  order, matching the fixture's own "derived-from-source, not
  live-captured" parsing discipline.
- `HR-TEST-004` A fake adapter implementation whose future never resolves
  proves the single flat 5 s external deadline (`HR-SAFE-002`) for each
  of `prompt`, `get`, and `list`: the crate returns
  `HerdrError::TimedOut` within the bound and the child is killed and
  reaped, asserted via the fake's own call record. This is the concrete
  shape of ADR-058 "Required evidence" for D10's steer/get bound, applied
  identically to `list`.
- `HR-TEST-005` A breaker fixture proves: three consecutive
  infrastructure-class failures open the breaker with exponentially
  increasing backoff capped at 30 s; a call while open returns
  `HerdrError::Unavailable` without spawning; and the first successful
  probe after `retry_after` elapses closes the breaker and resets the
  failure counter (ADR-058 D10.1 "Required evidence").
- `HR-TEST-006` No test in this crate, or in any consumer crate's test
  suite, invokes a live `herdr` binary or a live Herdr server. CI never
  depends on Herdr being installed. Live validation is a separate,
  manually-run transcript procedure (ADR-058 "Required evidence";
  `herdr-cli-contract-fixture.md` §F5) outside the automated test suite.

### 3.5 Version and Pin Policy

- `HR-VER-001` `atm-herdr` targets Herdr `0.8.2`, wire protocol `20`
  (ADR-058 "Pinned Herdr revision"). This crate's argv and parsing logic
  are derived from the Herdr source at checkout `d79fd746`, verified
  byte-identical to tag `v0.8.2` for every cited surface.
- `HR-VER-002` A Herdr release that changes any row of ADR-058's
  exit-code or error-code tables is a fix-forward event for this crate —
  never a silent pin bump. AQ6's ecosystem preflight re-runs
  `herdr-cli-contract-fixture.md` plus this crate's argv/stderr-parsing
  tests against the new pin before it is accepted.
- `HR-VER-003` This crate treats a Herdr server `PROTOCOL_VERSION`
  mismatch (`protocol_mismatch`) as a normal, typed `HerdrError` outcome,
  never a panic or an unrecoverable state; a version skew is expected to
  occur between a client upgrade and a server restart (ADR-058
  "Consequences").

## 4. Required References

The `atm-herdr` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`../atm-core/requirements.md`](../atm-core/requirements.md)
- [`../atm-core/architecture.md`](../atm-core/architecture.md)
- [`../adr/ADR-058-herdr-local-steer-backend-contract.md`](../adr/ADR-058-herdr-local-steer-backend-contract.md)
- [`../adr/ADR-054-nudge-taxonomy-and-queue-mechanism.md`](../adr/ADR-054-nudge-taxonomy-and-queue-mechanism.md)
- [`../plans/phase-aq/sprint-AQ1-queue-cli.md`](../plans/phase-aq/sprint-AQ1-queue-cli.md)
- [`../plans/phase-aq/sprint-AQ2-6-herdr-steer-backend.md`](../plans/phase-aq/sprint-AQ2-6-herdr-steer-backend.md)
- [`../plans/phase-aq/sprint-AQ2-7-herdr-queue-wake.md`](../plans/phase-aq/sprint-AQ2-7-herdr-queue-wake.md)
- [`../plans/phase-aq/fixtures/herdr-cli-contract-fixture.md`](../plans/phase-aq/fixtures/herdr-cli-contract-fixture.md)

## 5. Non-Goals

- `atm-herdr` does not implement a Herdr-native queue, an atomic
  idle-and-send operation, per-turn tracking, or any form of
  idempotency for repeated prompts (ADR-058 "Explicitly NOT relied
  upon"); these do not exist in the pinned Herdr release and this crate
  makes no claim otherwise.
- `atm-herdr` does not decide *when* to steer or queue a nudge. It has no
  knowledge of `NudgeKind`, `NudgeMode`, mail state, or roster
  eligibility; every call arrives with an already-resolved `AgentName`
  and an already-resolved optional session.
- `atm-herdr` does not run or wrap `agent start` or `agent rename`.
  These are operator/launch-convention commands (ADR-058 D6); the
  external team launcher runs them directly, not through this crate.
- `atm-herdr` does not support Herdr on Windows in Phase AQ (ADR-058
  "Explicitly NOT relied upon": named-pipe transport exists in Herdr but
  is out of scope here).
- `atm-herdr` does not retry a lifecycle-shaped failure
  (`agent_blocked`, `agent_not_found`, `agent_not_ready`,
  `agent_target_ambiguous`) itself; retry/requeue/release policy for
  those outcomes is caller-owned (`atm-storage`'s `PendingNudgeStore`,
  `atm-http-runtime`'s queue pump).
- `atm-herdr` does not decide queue-tick cadence, FIFO ordering, or which
  pending member to service next; `list`'s only job is to report every
  agent's current status for a given session, once per call. The AQ2.7
  pump — not this crate — decides how often to call `list`, how to group
  members by session, and how to pick the next `claim_next_pending`
  target.
- `atm-herdr` does not read or write `.atm.toml`, roster rows, or
  `mail_message_states`; it has no storage dependency of any kind.

## 6. Req-QA Verification Anchors

`req-qa` should treat these as fail-closed presence checks:

- `HR-CORE-001`–`HR-CORE-005`
  - `HerdrProcessAdapter` exists with exactly `prompt`, `wait`, `get`,
    `list` methods; a grep for `herdr` argv literals or Herdr JSON field
    names (`agent_status`, `error.code`, `agent_blocked`, …) outside
    `crates/atm-herdr` fails the source-audit gate (see
    `boundaries.md`)
  - argv-equality tests exist for all four emitted shapes and match
    `herdr-cli-contract-fixture.md` verbatim
  - a grep of `atm-http-runtime`'s `HerdrQueueWakePump` confirms it calls
    `list` and `prompt`, never `wait`, in Phase AQ
- `HR-CORE-006`
  - a fixture proves `HERDR_SESSION` is present on the child environment
    only when a `Some` session was passed, and absent (not empty-string)
    otherwise
- `HR-CORE-007`–`HR-CORE-008`
  - `AgentSnapshot` and `HerdrError` are both closed, non-`#[non_exhaustive]`
    within this crate's own tests, with one `HerdrError` variant per
    ADR-058 D8 table row plus the transport/protocol/timeout family
- `HR-SAFE-001`
  - no reference to `send-keys`, `send_input`, or `tmux` exists anywhere
    in `crates/atm-herdr`
- `HR-SAFE-002`/`HR-TEST-004`
  - the never-resolving fake-adapter fixture exists for the single flat
    5 s bound and asserts it applies to `prompt`, `get`, and `list`
    alike, and that the child is killed and reaped
- `HR-SAFE-003`
  - no function in this crate accepts a raw ATM message body as an
    argument to `prompt`
- `HR-SAFE-005`/`HR-SAFE-006`/`HR-TEST-005`
  - the breaker fixture in `HR-TEST-005` passes, `agent list` failure is
    exercised as an opening trigger, and a grep confirms no
    lifecycle-shaped `HerdrError` variant appears in
    `HerdrSpawnBreaker`'s open-triggering match arms
- `HR-TEST-006`
  - `cargo test -p atm-herdr` and every consumer crate's suite run with
    no `herdr` binary on `PATH` in CI and still pass
- `HR-VER-001`
  - the pin table in `ADR-058` and this crate's fixture-derived tests
    name the same `0.8.2` / protocol `20` pair
