# ADR-058 — Herdr Local Steer Backend Contract

| Field | Value |
| --- | --- |
| ID | ADR-058 |
| Status | Proposed (Phase AQ lane A; dispatch precondition for AQ2.6, critical review B9/I16) |
| Scope | `HerdrReceivedHook` (AQ2.6) and `HerdrQueueWakePump` (AQ2.7) process contract with the `herdr` CLI, implemented by the `atm-herdr` crate (`docs/atm-herdr/architecture.md`) |
| Relates to | ADR-001, ADR-054, ADR-056, sprint-AQ2-6, sprint-AQ2-7, sprint-AQ6 (pin-latest preflight) |

## Context

AQ2.6 adds Herdr as an explicit, alternate local message-received backend and
AQ2.7 adds a lifecycle-gated queue wake pump on top of it. Both sprints
hard-code `herdr agent ...` argv and depend on Herdr's structured error codes
(`agent_blocked`, `agent_not_found`) and exit semantics. Until this record the
repo carried no Herdr contract (critical review B9) and the planned argv did
not say how a team/workspace is selected (I16).

This ADR pins the Herdr release, records the exact argv atm-core emits, the
error-code and exit-code contract, the target-resolution rules Herdr actually
implements, and the behaviours atm-core explicitly does **not** rely on.
Every claim below is derived from the Herdr source at the pinned revision;
citations are `file:line` in the Herdr checkout. Nothing here was captured
from a live agent session.

### Pinned Herdr revision

| Item | Value |
| --- | --- |
| Installed binary | `/opt/homebrew/bin/herdr`, `herdr --version` = `herdr 0.8.2`, stable channel, wire protocol **20** (`herdr status`) |
| Source tag | `v0.8.2` (`git tag -l` in `/Users/randlee/Documents/github/herdr`) |
| Source checkout used for citations | `d79fd746` (`Cargo.toml` `version = "0.8.2"`), 35 commits after `preview-2026-08-19-b5c4a0176e91` |
| Checkout vs binary | Checkout is **newer** than the binary: `git diff --stat v0.8.2 HEAD` on the cited files shows only (a) `src/protocol/wire.rs:16` `PROTOCOL_VERSION` 20 → 21 and (b) an `agent explain --file` error-path change in `src/cli/agent.rs:118-135`. Every `agent prompt`/`wait`/`rename`/`start`, target-resolution, error-body and exit-code surface cited here is byte-identical between `v0.8.2` and `d79fd746`. Line numbers below are from `d79fd746`; in `v0.8.2` the `src/cli/agent.rs` lines after 120 are 14 lower. |

**Pin policy.** atm-core pins Herdr **0.8.2 / protocol 20** for Phase AQ.
AQ6's ecosystem preflight (`pin-latest` rule, sprint-AQ6) adds `herdr` to the
pinned-dependency table beside sc-compose/sc-observability/Wyvern: the
preflight verifies `herdr --version` against the recorded pin, bumps the pin
to the latest release, and re-runs the contract fixture in
`herdr-cli-contract-fixture.md` plus the AQ2.6/AQ2.7 command-construction and
stderr-parsing tests. A Herdr release that changes any row of the exit-code or
error-code tables below is a fix-forward event for the emitter/pump, never a
silent pin bump.

## Decision

### D1. Target grammar and workspace selection

Herdr's `agent.*` methods take one opaque `target: String`
(`src/api/schema/agents.rs:176-181`, `:26-32`, `:43-47`). Resolution for
`agent prompt`, `agent wait` (via `agent.get`), and `agent rename` is
`App::resolve_agent_target` (`src/app/terminal_targets.rs:75-106`):

1. If `target` parses as a **public pane id** of the form
   `<workspace_id>:p<N>` (`src/workspace.rs:145-147`,
   `src/app/ids.rs:145-151`) **and** that pane currently hosts an agent
   terminal, it resolves to that pane (`terminal_targets.rs:79-86`).
2. Otherwise Herdr scans **every pane in every workspace of the server**
   (`terminal_targets()`, `:136-157`) for terminals whose `agent_name` is an
   exact string match (`:88-98`).
3. Exactly one match → resolved. Zero → `agent_not_found`. More than one →
   `agent_target_ambiguous` with a candidate list naming
   `terminal_id/pane_id/workspace_id/tab_id/cwd/status`
   (`terminal_targets.rs:116-134`, `src/app/agents.rs:290-318`).

Consequences atm-core adopts:

- **Agent names are global per Herdr server, not per workspace.** `agent
  start` and `agent rename` refuse a name already used by any agent in any
  workspace with `agent_name_taken` (`src/app/agents.rs:104-111`, `:163-169`,
  conflicts collected over all workspaces at `:399-410`). Herdr therefore
  guarantees that a bare `<AgentName>` target is unambiguous *among named
  agents*. (Ambiguity can only arise if an unnamed agent's auto-detected
  label happens to equal a name — see "Not relied upon".)
- **There is no `--workspace`, `--session`, or `workspace/agent` target
  syntax on any `herdr agent` subcommand** (`src/cli/agent.rs:506-560`,
  `:751-769`, `:771-841`; the only options parsed are `--wait`, `--until`,
  `--timeout`, `--clear`). Workspace id is a *result* field
  (`AgentInfo.workspace_id`, `src/api/schema/agents.rs:208`), never an input.
- The only server/session selector is the **environment**:
  `HERDR_SOCKET_PATH` (explicit socket, `src/api/mod.rs:20`,
  `src/session.rs:173-180`) or `HERDR_SESSION=<name>` which selects
  `<config_dir>/sessions/<name>/herdr.sock` (`src/session.rs:96-101`,
  `:161-171`). Default is `~/.config/herdr/herdr.sock`. A named session is a
  *separate server process*, not a workspace.

**Resolution of I16 (workspace/team selection):** atm-core emits the member
`AgentName` (= `ATM_IDENTITY`) as the bare `<TARGET>` and emits **no**
workspace/team argument because Herdr has none. `ATM_TEAM` is **not**
expressible on the Herdr argv. The launch convention in AQ2.6 becomes:

> The Herdr agent MUST be started or renamed so its live agent name equals
> `ATM_IDENTITY`. Because Herdr names are unique per server, two teams on one
> Herdr server cannot both have a member named `team-lead`; operators who
> need that must run each team in its own Herdr session and record that
> session on the affected members' roster rows (below) — the session is
> never read from, or exported into, the atm daemon's own process
> environment.

**Session sourcing — one model.** `HerdrSession` and the field that carries
it, `LocalMessageReceivedBackend::Herdr { session: Option<HerdrSession> }`,
are owned and defined by AQ1
(`docs/plans/phase-aq/sprint-AQ1-queue-cli.md`, "Trait-foundation scope";
landed at `crates/atm-core/src/delivery_channel.rs`), **not** by this ADR.
ADR-058 does not define, redefine, or widen that type; it only fixes how the
Herdr emitter *consumes* the `session` field on each invocation. Because the
daemon never launches Herdr sessions — the external team launcher does — the
session an agent lives in is **roster data per member**, exactly as
`recipient_pane_id` is for tmux, set at `atm teams add-member … --backend
herdr [--session <name>]` (AQ2.6), `None` meaning Herdr's default server.
The Herdr emitter sets `HERDR_SESSION=<session>` on the **child process
environment, per invocation**, only when the member's stored `session` is
`Some`; when it is `None`, `HERDR_SESSION` is left unset on the child. The
daemon's own environment is never consulted for this value, and atm-core
never synthesises a session name from any other source. AQ2.6's sprint doc
(`docs/plans/phase-aq/sprint-AQ2-6-herdr-steer-backend.md`) already reflects
this exact type and per-invocation rule.

Consequences: one daemon serves teams in different sessions; name uniqueness
is per session, so same-named members in different teams are fine when their
teams run in different sessions; a member whose stored session does not
match where its agent actually runs surfaces as `agent_not_found`, which
`atm doctor` (AQ2.6) reports as "agent not visible in the member's
configured Herdr session" by calling `herdr agent get <name>` under that
same per-invocation env (D9).

**History (2026-08-26).** An earlier draft of this section had the daemon
inherit a shared `HERDR_SESSION`/`HERDR_SOCKET_PATH` from its own process
environment ("one shared session default / escape hatch"). That framing
contradicted the per-member model above and was never a design this ADR
should have carried forward; it is superseded in full by the per-member
`HerdrSession` roster field, which is the only model this ADR states.

The "workspace equals `ATM_TEAM`" clause in AQ2.6 is advisory operator
practice only; atm-core neither checks nor depends on it.

**Name grammar constraint.** Herdr accepts names matching
`^[a-z][a-z0-9_-]{0,31}$` only (`src/app/agents.rs:13-20`); anything else is
`invalid_agent_name` on start/rename. AQ2.6's `--backend herdr` path must
reject (or at least warn on) an `ATM_IDENTITY` outside this grammar at
`add-member`/`update-member` time, because such a member can never be
targeted.

This grammar recurs across D1/D2/D4-D8 and the fixture as bare, ad hoc
prose. AQ2.6, which owns the `add-member`/`update-member` validation call
site, SHOULD wrap it in a validated newtype (`HerdrAgentName`, precedent:
`HerdrSession::new` in `delivery_channel.rs`) rather than re-validating a
raw `String`/`&str` at each call site; this ADR does not itself define that
type (AQ1 owns `delivery_channel.rs`), it only records the recommendation so
AQ2.6 does not have to re-derive it.

### D2. Exact argv atm-core emits

All invocations are direct `execve` of `herdr` with separate argv elements,
no shell, no `--wait`, no message body. Three shapes are emitted by
atm-core; a fourth (`agent wait`) is documented for completeness but is
**not emitted by atm-core in Phase AQ**.

**Immediate steer (AQ2.6, `HerdrReceivedHook`):**

```text
herdr agent prompt <AgentName> "You have unread ATM messages. Run: atm read"
```

- `<AgentName>` is argv[3] verbatim (no quoting layer; `src/cli/agent.rs:771-781`).
- argv[4] is the fixed mailbox-read text. It must be non-empty
  (`empty_agent_prompt`, `src/app/api/agents.rs:63-65`).
- No `--wait`. Without `--wait`, `--until`/`--timeout` are usage errors
  (exit 2, `src/cli/agent.rs:824-831`), so the steer argv carries neither.

**Queue wake gate — polling probe (AQ2.7, `HerdrQueueWakePump`), followed by
the same steer argv above for each member the poll observes `idle`/`done`
(D9.1):**

```text
herdr agent list
```

`agent list` takes no arguments (`src/cli/agent.rs:438-441`: any argv beyond
the bare subcommand is a usage error, exit 2). One call is made per distinct
Herdr session, each tick of AQ2.7's poll — never one call per member. See
D9.1 for the full contract (request/response shape, session env, errors).

**Documented, NOT emitted by atm-core in Phase AQ — `agent wait`.** The
prior AQ2.7 draft (superseded 2026-08-26, Rand) used this instead of
polling:

```text
herdr agent wait <AgentName> --until idle --until done --until blocked --timeout <ms>
```

- `--until idle --until done --until blocked` is exactly Herdr's default set
  (`src/api/wait.rs:511-523`); it is spelled out so the gate's intent is
  explicit and independent of a future default change. `unknown` is
  deliberately excluded.
- `--timeout <ms>` is a `u64` millisecond count (`src/cli.rs:928-932`); no
  upper bound is enforced by Herdr.

This argv shape, and D5 below, remain accurate documentation of what
`herdr agent wait` does — `HerdrProcessAdapter::wait` stays defined on the
trait (`atm-herdr`) so the contract stays compiled and testable — but no
Phase AQ sprint constructs or spawns this argv. A future phase that needs a
true lifecycle-blocking gate rather than a poll may revive it; until then it
is dead code by design, not an oversight.

Optional operator/launch convenience (not emitted by the daemon, documented
for the launch convention only):

```text
herdr agent start <AgentName> --kind <claude|codex|hermes|...> --pane <workspace_id>:p<N> [--timeout <ms>] [-- <agent-args>...]
herdr agent rename <TARGET> <AgentName>
```

### D3. Transport and process contract

- Every `herdr agent ...` command opens a fresh connection to the socket,
  first pings for protocol compatibility (`src/cli.rs:762-797`), then sends
  one newline-delimited JSON request (`src/api/client.rs:55-61`). There is
  **no client-side read timeout** on `send_request`; a `wait` blocks on the
  socket until the server answers.
- **stdout**: exactly one JSON line on success (`src/cli.rs:744`).
  **stderr**: exactly one JSON line `{"id":"<req>","error":{"code":"…","message":"…"}}`
  on any structured error (`src/cli.rs:739-742`, `src/main.rs:572-578`), or a
  plain-text usage line on argv errors. Request ids are fixed:
  `cli:agent:prompt`, `cli:agent:wait`, `cli:agent:rename`, `cli:agent:start`.
- **Exit codes** (`src/cli.rs:738-746`, `src/main.rs:568-580`):

| Exit | Meaning | Source |
| --- | --- | --- |
| 0 | success; stdout JSON | `cli.rs:744-745` |
| 1 | structured error; stderr JSON with `error.code` (incl. `server_not_running`, `protocol_mismatch`) | `cli.rs:739-742`, `main.rs:571-579` |
| 2 | argv/usage error; plain text on stderr, no JSON | `cli/agent.rs:772-831`, `:506-551` |
| other/non-zero via `Err` | raw I/O error propagated from `main` (only for non-connect I/O failures) | `main.rs:580` |

- **Server down**: connect fails with `NotFound`/`ConnectionRefused` →
  stderr `{"id":"cli:agent:prompt","error":{"code":"server_not_running","message":"no herdr server is running at <socket>; run `herdr` to start or attach it"}}`, exit 1
  (`src/cli.rs:819-844`, `src/cli/server_not_running.rs:28-40`, `src/main.rs:572-578`).
- **Version skew**: the client compares its `PROTOCOL_VERSION`
  (`src/protocol/wire.rs:16`) with the server's `Pong.protocol`
  (`src/api/schema/response.rs:45-50`) on every command; inequality yields
  stderr `{"…","error":{"code":"protocol_mismatch",…}}`, exit 1
  (`src/cli/protocol_guard.rs:16-43`, `src/cli.rs:777-797`). There is no
  separate API schema version; `herdr api schema` prints the bundled JSON
  schema and the `protocol` integer is the only compatibility token. Any
  client/server mismatch is fatal (strict equality), so a Herdr upgrade
  requires a server restart before atm nudges work again.

### D4. `agent prompt` semantics

Server handler `App::handle_agent_prompt` (`src/app/api/agents.rs:62-131`),
checks in order, all **before any byte is written**:

| Order | Condition | `error.code` | Line |
| --- | --- | --- | --- |
| 1 | empty text | `empty_agent_prompt` | 63-65 |
| 2 | target resolution fails | `agent_not_found` / `agent_target_ambiguous` | 66-69, `app/agents.rs:290-318` |
| 3 | pane has no terminal | `agent_not_found` | 70-81 |
| 4 | terminal `state == Blocked` | `agent_blocked` (`"agent <T> is blocked and requires interactive input"`) | 82-91 |
| 5 | no known agent kind, or managed launch still pending | `agent_not_ready` | 92-97 |
| 6 | no live PTY runtime | `agent_not_found` | 98-100 |
| 7 | agent no longer the pane foreground process | `agent_not_ready` | 101-110 |
| 8 | PTY write of text fails | `agent_prompt_failed` | 121-125 |

On success the text is written immediately and **Enter is scheduled by the
server 300 ms later** (`AGENT_PROMPT_SUBMIT_DELAY`, `:13`, `:126`), then
`{"result":{"type":"agent_prompted","agent":{…}}}` is returned. "Atomic" means:
one server-side request performs the text write and Enter without client
involvement, and the blocked check precedes the write; it does **not** mean
the Enter has been delivered when the CLI exits. A successful exit therefore
means *submission accepted*, nothing more (matches AQ2.6 deliverable 3).

Codes reachable from `agent prompt` without `--wait`: `empty_agent_prompt`,
`agent_not_found`, `agent_target_ambiguous`, `agent_blocked`,
`agent_not_ready`, `agent_prompt_failed`, plus transport-level
`server_not_running`, `protocol_mismatch`, `server_unavailable`
(`src/api/server.rs:371-377`, `:843-847`), `internal_error`.

With `--wait` (not used by atm-core; recorded for completeness,
`src/api/wait.rs:177-306`): a pre-prompt `agent.get`, the prompt, then if the
agent was not `working` an "effect" gate of `min(--timeout, 5000)` ms that
must observe a `state_change_seq` advance, else `agent_prompt_stalled`
(`:20`, `:232-277`, `:620-631`) — or plain `timeout` if `--timeout <= 5000`
(`:238-242`); then a settled wait on `--until` (default idle/done/blocked)
producing `timeout` or `agent_not_running`. AQ2.6/AQ2.7 never add `--wait`,
so `agent_prompt_stalled` is unreachable from atm-core.

### D5. `agent wait` semantics

**Not invoked by atm-core in Phase AQ (D2).** Retained here because
`HerdrProcessAdapter::wait` stays defined on the trait and this is its
compiled contract; AQ2.7's queue pump uses D9.1's `agent list` polling
probe instead. A future phase reviving `wait` inherits this section
unchanged.

CLI `src/cli/agent.rs:506-560`; server `src/api/wait.rs:132-175`, loop at
`:348-498`.

- State vocabulary: `idle | working | blocked | done | unknown`
  (`src/api/schema/common.rs:149-157`, snake_case). Default match set when no
  `--until`: `idle, done, blocked` (`wait.rs:511-523`). `unknown` matches only
  when explicitly listed.
- Initial `agent.get` runs first: a missing name is `agent_not_found`
  immediately (`wait.rs:141-148`); an agent already in a matching state
  returns success at once (`:150-152`) without observing a transition.
- Success: stdout `{"id":"cli:agent:wait","result":{"type":"agent_info","agent":{…,"agent_status":"idle",…}}}`, exit 0. atm-core reads `result.agent.agent_status`.
- Timeout: stderr `{"id":"cli:agent:wait","error":{"code":"timeout","message":"timed out waiting for agent status"}}`, exit 1 (`wait.rs:470-495`, `:616-619`). The deadline check re-probes once before failing, so a state reached exactly at deadline still succeeds.
- Agent disappears / pane closes / pane moves / different agent detected mid-wait: `agent_not_running` (`wait.rs:396-436`, `:643-659`); an `agent_not_found` probe result is translated to `agent_not_running` (`:654-657`).
- Without `--timeout` the wait is indefinite (`:356-358`). atm-core always passes `--timeout`.
- Cancellation: the `herdr` client installs no signal handler; SIGTERM/SIGKILL
  kills it and closes the socket. The server loop polls
  `should_stop_connection` every 100 ms (`wait.rs:371`,
  `src/api/server.rs:28`, `:782-791`) and abandons the wait with no side
  effects. Killing the child is therefore a clean cancel; no input is ever
  written by `wait`.

### D6. `agent rename` and `agent start`

- `herdr agent rename <TARGET> <NAME>|--clear` (`src/cli/agent.rs:751-769`;
  server `src/app/agents.rs:90-143`). `TARGET` follows D1 (pane id or current
  name). NAME must match the D1 grammar (`invalid_agent_name`), be unused
  server-wide (`agent_name_taken`), the terminal must host a detected agent
  (`agent_not_found` with message "agent target does not currently host an
  agent", `:127-128`, `:325-328`) and not be mid-startup
  (`agent_launch_pending`, `:124-126`). The new name takes effect immediately
  for target resolution (`set_agent_name`, `:131`) and is persisted in the
  session; it does not change the pane label.
- `herdr agent start <NAME> --kind <KIND> --pane <ID> [--timeout MS] [-- args…]`
  (`src/cli/agent.rs:289-436`; server `src/app/agents.rs:145-227`). `--pane`
  is **required** and must be an existing pane sitting at an interactive
  shell prompt. Supported kinds include `claude`, `codex`, `hermes` (help
  output; `src/detect`). Server errors: `invalid_agent_name`,
  `unsupported_agent_kind` (CLI pre-rejects with exit 2), `agent_name_taken`,
  `agent_pane_not_found`, `agent_pane_busy`, `agent_pane_unavailable`,
  `agent_start_input_failed`, invalid timeout (must be >3000 and ≤300000 ms,
  `schema/agents.rs:170`). The CLI then polls until the named agent is
  `idle|done` with `interactive_ready`, returning `timeout`,
  `agent_kind_mismatch`, `agent_name_not_found`, `agent_not_ready`, or
  `agent_start_failed` (`src/cli/agent.rs:562-632`). Success prints
  `{"result":{"type":"agent_started","agent":{…},"argv":[…]}}`.

atm-core never runs `start` or `rename`; they are part of the operator launch
convention only.

### D7. Acknowledged list→prompt race (AQ2.7)

`agent list` observing a member's `agent_status` as `idle`/`done` (D9.1) and
the subsequent `agent prompt` reaching the PTY are two separate socket
requests, separated by the pump's own claim/dispatch work in between (AQ1's
`claim_next_pending`, then `rebuild_received_hook_dispatch`, then the
selector call into `HerdrReceivedHook`). The agent may start a turn or enter
a blocked dialog in that interval. The contract offers no atomic
"list-then-prompt" or "wait-then-prompt" method (`Method` enum,
`src/api/schema.rs:39-236`, has no such variant). The final guard is `agent
prompt`'s own `agent_blocked` pre-write check (D4 row 4); a prompt landing
on a *working* agent is accepted by Herdr and is an accepted limitation, not
a defect. This is the same class of race the prior `agent wait`-based draft
carried (a `wait` return and a `prompt` call were likewise two separate
requests) — polling does not introduce a new race, it only widens the
window from "process spawn time" to "up to one poll interval." AQ2.7 must
record this in events and the ADR-054 addendum.

### D8. Error-code contract atm-core parses

| `error.code` | Emitting command(s) | atm-core outcome | Cause | Recovery |
| --- | --- | --- | --- | --- |
| `agent_blocked` | prompt | `blocked_before_input` (AQ2.6) / `release_pending` (AQ2.7, D7's list→prompt race) | target agent is at an approval/question UI; Herdr rejected before writing any input | no retry on this path; durable mail stays readable via `atm read`; operator/agent clears the blocking UI, a later poll tick (or manual `atm read`) delivers |
| `agent_not_found` | prompt, rename | `target_not_present` (AQ2.6) / `held_target_not_present` (AQ2.7, post-claim prompt only — see the new list-based row below for AQ2.7's pre-claim case) | no live agent named `<AgentName>` on the resolved Herdr server/session (renamed, exited, wrong session) | mail already persisted and stays readable; operator runs `atm doctor` / `herdr agent list`, fixes the member's `--session` or restarts the agent under its configured name |
| member's `AgentName` absent from `agent list`'s returned `agents[]` (no error code — a normal, name-less exit-0 result; D9.1) | list | `held_target_not_present`, pre-claim (AQ2.7 only) | agent not currently detected under that name in the polled session — same underlying condition `agent_not_found`/`agent_not_running` cover for `get`/`prompt`, observed instead as an absence in a list rather than a per-target error | no claim was ever taken, so nothing to release; doctor-visible; re-evaluated automatically on the next tick (`HERDR_POLL_INTERVAL_MS`), no separate backoff state |
| `agent_not_running` | *(historical — reachable only from `wait`, which D2 documents but atm-core does not invoke in Phase AQ)* | would be `held_target_not_present` if `wait` were ever emitted | agent exited, its pane closed, or it was renamed away while a wait was outstanding | superseded in Phase AQ by the absent-from-list row above, which AQ2.7 actually exercises |
| `agent_target_ambiguous` | prompt, rename *(also reachable from `wait`, not invoked in Phase AQ)* | advisory failure, no retry (operator must fix names) | two or more terminals resolve to the same name (stale/duplicate `agent_name`) | operator renames the duplicate agent(s); atm-core has no automatic disambiguation |
| `agent_not_ready` | prompt | advisory failure; `release_pending` when post-claim (AQ2.7) | agent still launching, or no longer the pane foreground process | AQ2.7 releases the claim for re-evaluation on the next tick; Steer has no retry mechanism and only logs a warning |
| `timeout` | *(historical — Herdr's own `wait`-scoped error code; D2 documents it but atm-core does not invoke `wait` in Phase AQ)* | not reachable in Phase AQ | agent never reached `idle`/`done`/`blocked` within a `wait`'s `--timeout` | AQ2.7's own notion of "took too long" is the D10 external child-process bound on `list`/`prompt`, which routes to the D10.1 breaker, not this code |
| `agent_prompt_failed`, `empty_agent_prompt`, `internal_error`, `server_unavailable` | prompt | advisory failure (`requeue_pending` for AQ2.6 immediate steer — no `PendingNudgeStore` claim exists on that path; `release_pending` for AQ2.7's post-claim prompt, since no input was injected) | PTY write failed; fixed prompt text was empty (should be impossible by construction); Herdr-internal error; server mid-shutdown | retry/re-evaluate; `empty_agent_prompt` specifically indicates an atm-core defect (D2's fixed text is never empty) and must also be logged as a bug |
| `server_not_running`, `protocol_mismatch` | any, including `list` | advisory failure, health counter `herdr_unavailable`; for AQ2.6/AQ2.7 this is one of the three D10.1 breaker triggers (alongside the D10 external-timeout kill) — the breaker opens host-wide, not per-caller | no Herdr server at the resolved socket; or client/server `PROTOCOL_VERSION` mismatch | operator starts/restarts the Herdr server (restart required after a Herdr upgrade for `protocol_mismatch`); `atm doctor` surfaces the `herdr_unavailable` health counter and the D10.1 breaker state |
| (exit 2, no JSON) | any | atm-core bug: argv construction error, must be impossible by construction | malformed argv (missing/extra positional, disallowed flag combination) | not operator-recoverable; a fixture/unit-test regression that must fail CI before reaching a real invocation |

Unknown codes are advisory failures; atm-core never matches on `message`.

### D9. `agent get` semantics (doctor probe)

CLI `src/cli/agent.rs:20` (dispatch), `:450-465` (`fn agent_get`); server
`handle_agent_get`, `src/app/api/agents.rs:25-32`.

```text
herdr agent get <AgentName>
```

- Exactly one positional `<target>`; zero, or two-or-more, args is a usage
  error, plain text on stderr, exit `2` (`src/cli/agent.rs:450-457`).
- Request id `cli:agent:get`, `Method::AgentGet(AgentTarget { target })`
  (`src/cli/agent.rs:459-465`, `src/api/schema.rs:109`).
- Target resolution is the **same** `App::resolve_agent_target` as D1
  (`src/app/terminal_targets.rs:75-106`) via `agent_info_for_target`
  (`src/app/agents.rs:64-73`) — there is no separate resolution rule for
  `get`.
- Success: stdout `{"id":"cli:agent:get","result":{"type":"agent_info","agent":{…}}}`,
  exit `0` (`handle_agent_get` returns `ResponseResult::AgentInfo`, printed
  by `src/cli.rs:738-746`). `AgentInfo` fields are D1's cited struct
  (`src/api/schema/agents.rs:184-223`, including `agent_status` and
  `workspace_id`); atm-core's doctor probe reads only `agent_status` — the
  "Explicitly NOT relied upon" exclusion of other `AgentInfo` fields and
  key ordering applies here too.
- Errors reachable from `get` are exactly the two `TerminalTargetError`
  variants mapped by `agent_target_error_body`
  (`src/app/agents.rs:288-317`): `agent_not_found` (no match) and
  `agent_target_ambiguous` (multiple matches), both stderr JSON, exit `1`.
  `agent_blocked`, `agent_not_ready`, and the other `agent prompt`-only
  codes (D4) **cannot** be returned by `get` — it performs no write and no
  PTY/foreground check. The universal transport codes
  (`server_not_running`, `protocol_mismatch`, D3) apply exactly as for
  every other command.
- `agent get` is Herdr's only true read-only agent probe: it has no
  `--wait`/`--timeout` of its own, so the doctor call site is bound solely
  by atm-core's own D10 child-process deadline, never by anything
  Herdr-side.

Any doctor-probe claim not sourced above (for example, a relationship
between `agent get` and `agent_session`/`state_labels`/`tokens`) is **not
contracted**; atm-core must not assert on it.

#### D9.1 `agent list` semantics (AQ2.7 queue-pump polling probe)

CLI dispatch `src/cli/agent.rs:19` (`"list" => agent_list(&args[1..])`),
`fn agent_list` at `src/cli/agent.rs:438-448`; server `handle_agent_list`,
`src/app/api/agents.rs:16-22`, backed by `collect_agent_infos`,
`src/app/agents.rs:22-35`. Module layout for the atm-core-side adapter that
consumes this contract is normatively documented in
`docs/atm-herdr/architecture.md` (crate `atm-herdr`), not here.

```text
herdr agent list
```

- **No arguments.** Any argv beyond the bare subcommand is a usage error,
  plain text on stderr, exit `2` (`src/cli/agent.rs:439-442`). There is no
  target, no `--session`/`--workspace` flag, and no pagination — consistent
  with D1's "no `--workspace`/`--session`/`workspace/agent` syntax on any
  `herdr agent` subcommand."
- Request id `cli:agent:list`, `Method::AgentList(EmptyParams::default())`
  (`src/cli/agent.rs:444-447`, `src/api/schema.rs:107`).
- Success: stdout one JSON line,
  `{"id":"cli:agent:list","result":{"type":"agent_list","agents":[…]}}`,
  exit `0`. `ResponseResult` is `#[serde(tag = "type", rename_all =
  "snake_case")]` (`src/api/schema/response.rs:43-44`), and `AgentList {
  agents: Vec<AgentInfo> }` (`:106-108`) is the variant, hence the literal
  `"type":"agent_list"` discriminant.
- **Scope: server-wide, not team- or session-scoped by any argument.**
  `collect_agent_infos` (`src/app/agents.rs:22-35`) iterates every
  workspace, every tab, every pane on the connected server and returns an
  `AgentInfo` for each pane hosting a detected agent terminal — named and
  unnamed alike. The **only** way atm-core narrows this to "one Herdr
  session" is the same environment-mediated socket selection every other
  command uses (D1): `HERDR_SESSION=<session>` set on the **`agent list`
  child specifically**, only when the polled roster population's stored
  session is `Some`, resolved by `active_api_socket_path`
  (`src/api/mod.rs:93-95` → `src/session.rs:96-101`, `:161-180`) before any
  request is sent — `agent list` carries no session argument of its own;
  the env var determines which server socket the whole CLI process
  connects to, uniformly for every subcommand, `list` included.
- **Fields atm-core relies on** (`AgentInfo`, `src/api/schema/agents.rs:184-223`):
  `name: Option<String>` (`:187`, `#[serde(skip_serializing_if =
  "Option::is_none")]` — omitted, not `null`, for an unnamed/undetected
  agent) and `agent_status: AgentStatus` (`:198`, required; snake_case
  `idle | working | blocked | done | unknown`,
  `src/api/schema/common.rs:151-157`). Unlike `get`/`prompt`, which take an
  explicit target and rely on nothing but `agent_status` ("Explicitly NOT
  relied upon" below), `list` additionally requires atm-core to rely on
  `name` — the only way to join a returned entry back to a polled
  `AgentName` — because `list` performs no filtering of its own. An entry
  with `name: None` can never match a configured member and is skipped.
  Every other `AgentInfo` field (`workspace_id`, `tab_id`, `pane_id`,
  `tokens`, `agent_session`, …) is out of contract for `list`, same as for
  `get`.
- **Errors.** `list` performs no target resolution (there is no target), so
  none of `agent_not_found`/`agent_target_ambiguous`/`agent_blocked`/
  `agent_not_ready` are reachable from it — a member simply not appearing
  in `agents[]` is not an error response, it is a normal exit-0 result with
  that name absent (D8's new "absent from `agent list`" row). Only the
  universal transport codes apply: `server_not_running`, `protocol_mismatch`
  (D3), and `internal_error`/`server_unavailable`.
- `agent list` has no `--wait`/`--timeout` of its own, so — like `get` — the
  call site is bound solely by atm-core's own D10 child-process deadline
  (the list bound, not the removed wait bound), never by anything
  Herdr-side.

### D10. Child-process bound (external timeout, independent of Herdr)

Herdr's own client applies **no** read/connect timeout on `send_request`
(D3): a `prompt` or a `list` waits on the socket until the server answers.
Every `herdr` child spawn is therefore wrapped in an **external,
atm-core-owned deadline**, independent of and in addition to any
`--timeout` argv value. Module layout for where this bound lives (beside
the adapter, `atm-herdr`) is normatively documented in
`docs/atm-herdr/architecture.md`.

- **Mechanism:** `tokio::time::timeout` around the child's wait-for-exit
  (the `Child::wait`/`Command::status` future), never around the spawn
  call itself. On elapse: kill the child (`Child::kill`) and reap it
  (await the kill's own wait) before returning `HerdrTimedOut` — a
  killed-but-unreaped child is a defect, not an acceptable outcome.
- **Steer/list bound (`prompt`, AQ2.6, `HerdrReceivedHook`; `list`, AQ2.7,
  `HerdrQueueWakePump`):** 5 s for both. `agent prompt` without `--wait`
  returns as soon as the server schedules the text write (D3, D4); `agent
  list` (D9.1) is a single request/response with no server-side matching
  loop at all — there is no legitimate reason for either round trip (ping
  plus one request/response) to exceed low single-digit seconds. For
  `prompt`, this bound sits inside the inherited `RequestDeadline` the
  emitter already awaits everything against
  (sprint-AQ2-6-herdr-steer-backend.md deliverable 3); it does not replace
  that deadline. For `list`, AQ2.7's tick task synthesizes its own
  `RequestDeadline::after(<5 s>)` per call, since a background poll has no
  inbound request to inherit one from.
- **Wait bound — removed 2026-08-26 (Rand).** The prior draft's
  per-member `wait` deadline (`HERDR_WAIT_GRACE_MS = 5_000` beside the
  `--timeout` argv value, up to 45 minutes total) is deleted along with
  AQ2.7's `agent wait` usage (D2). `HERDR_WAIT_GRACE_MS` no longer exists
  as a named constant. Only the steer/list bound above remains.
- Cancellation (SIGTERM/SIGKILL) on the steer/list bound is a clean cancel
  from Herdr's side (D3, D7's list→prompt race note): a killed `list`
  writes nothing (it is read-only), and a killed `prompt` before its write
  has landed leaves no partial state (the write is one server-side atomic
  step, D4).
- **Required evidence.** A fake `HerdrProcessAdapter` implementation whose
  future never resolves (never exits) proves the emitter/pump returns a
  distinct `HerdrTimedOut` outcome within the 5 s bound above and that the
  child is killed, asserted via the fake's own call record — this is the
  concrete shape of sprint-AQ2-6-herdr-steer-backend.md AC 11 ("Deadline/
  cancellation tests prove no child process or background task survives
  the request"). AQ2.7 owns the equivalent test for `list`, not `wait`.

## Consequences

- AQ2.6's persisted representation stays mode-only; the live target is
  `ATM_IDENTITY` and nothing else. The doctor/CLI must validate the name
  grammar `^[a-z][a-z0-9_-]{0,31}$`.
- `ATM_TEAM` cannot be selected on the Herdr argv; per-team isolation on a
  shared host requires per-team Herdr sessions, recorded per member on the
  roster (D1) — never via the daemon's own process environment. This is a
  documented operator constraint, not an atm-core feature.
- Both sprints parse one JSON line from stderr and switch on `error.code`;
  exit code 1 alone is not sufficient to classify.
- A Herdr upgrade that changes `PROTOCOL_VERSION` (already 21 at `d79fd746`)
  turns every nudge into `protocol_mismatch` until the server is restarted;
  AQ6's preflight must treat the checkout-vs-binary protocol skew as a
  blocking finding.

## Explicitly NOT relied upon

- Workspace, tab, or session identity of the target — never inspected.
- The "workspace equals `ATM_TEAM`" convention — advisory only.
- Uniqueness of auto-detected labels (`effective_agent_label`) — resolution
  by label is a `resolve_terminal_target` feature (`terminal_targets.rs:62`)
  that `resolve_agent_target` does **not** use; only exact `agent_name` counts.
- Delivery of the 300 ms delayed Enter, the agent reading the mailbox, a turn
  completing, or `done`/`idle` meaning "message read".
- `--wait`, `agent_prompt_stalled`, `agent send-keys`, `pane send-keys`,
  `pane send-input`, `events.subscribe`/`events.wait`, `herdr api snapshot`.
- Any Herdr-side queue, per-turn tracking, or idempotency of repeated prompts.
- Herdr on Windows (named-pipe transport exists but is out of AQ scope).
- Text of `error.message`, `AgentInfo` fields other than `agent_status`,
  and the ordering of JSON keys.

## Rejected alternatives

1. **Target by pane id `<workspace_id>:p<N>`** — stable within a session but
   persisted state again (the AQ2.6 pre-review `HerdrAgentTarget` mistake);
   names are live and self-healing on rename.
2. **Add a `--workspace` flag to Herdr** — upstream change, outside Phase AQ;
   global name uniqueness already gives an unambiguous target.
3. **`agent prompt --wait --until idle` as an atomic idle-gated send** —
   `--wait` waits *after* submission; it cannot defer the write, and adds the
   5 s `agent_prompt_stalled` gate that would misclassify fire-and-forget.
4. **Fall back to `agent send-keys` on `agent_not_ready`** — reintroduces the
   raw-key path AQ2.6 forbids and bypasses the blocked check.
5. **Parse `herdr status` for readiness before each nudge** — redundant; every
   command already pings and yields `server_not_running`/`protocol_mismatch`.

## Required evidence

- `herdr-cli-contract-fixture.md` (this lane) committed beside this ADR and
  consumed by AQ2.6/AQ2.7 fixture tests; each fixture row marked
  derived-from-source until AQ2.6's live macOS/Linux validation replaces it
  with captured output.
- AQ2.6 tests: argv equality for the steer command; stderr parse of each row
  in D8; exit 2 impossible-by-construction test; the D10 fake-adapter
  never-exits test proving `HerdrTimedOut` within the 5 s steer bound and a
  killed child (sprint-AQ2-6-herdr-steer-backend.md AC 11).
- AQ2.7 tests: argv equality for the `list` command, including per-session
  grouping and correct `HERDR_SESSION` assignment per child (D9.1); idle/done
  gating parsed from `agent_status` in `list`'s response; absent-from-list
  and post-claim `agent_blocked` release gating (D7, D8); child kill cleans
  up with no prompt emitted; the D10 fake-adapter never-exits test for
  `list`'s 5 s bound, not a `wait` bound (removed).
- AQ6: `herdr --version` and `herdr status` protocol recorded in the pin
  table; preflight fails on binary/server protocol mismatch.


#### D10.1 Spawn backoff when Herdr is unavailable (RSH-003)

`server_not_running`, `protocol_mismatch`, and an external-timeout kill are
*infrastructure* outcomes: retrying the spawn immediately cannot succeed and
burns a child process per attempt. **An `agent list` failure is the same
class of outcome as an `agent prompt` failure here** — both are single
bounded child spawns behind the same socket (D9.1, D10) — so atm-core
applies one **per-host circuit breaker** shared by every Herdr member and
every Herdr command (the Herdr server is host-wide):

- State lives beside the adapter in `atm-herdr` (structural change,
  2026-08-26: not `atm-http-runtime` — see D2/D9.1/D10 and
  `sprint-AQ2-6-herdr-steer-backend.md` deliverable 3), `HerdrSpawnBreaker`,
  composition-root singleton; never in the roster or SQLite. Module layout
  is normatively documented in `docs/atm-herdr/architecture.md`.
- On any of the three outcomes — including one from a `list` call — the
  breaker opens for `backoff = min(1 s × 2^consecutive_failures, 30 s)`;
  while open, `prompt`/`get`/`list` return `HerdrUnavailable { retry_after }`
  **without spawning**. (`wait` would too, if it were ever invoked — it is
  defined on the trait per D2 but not called by atm-core in Phase AQ.)
  Steer-kind nudges are dropped with a structured event
  (`subsystem="atm_core.herdr" action="steer_skipped_breaker_open"`);
  queue-kind claims — including every claim AQ2.7 took for a session whose
  `list` call fails this tick — are released via `release_pending` (no
  retry-budget consumption — nothing was injected).
- The first successful spawn after `retry_after` closes the breaker and
  resets the counter; a single probe is allowed through when `retry_after`
  elapses (half-open), so recovery is detected within one backoff window.
- `atm doctor` reports the breaker state (`herdr_breaker: closed | open
  {retry_after_ms, consecutive_failures}`) as a Warning while open — AQ2.7's
  own `herdr_queue_pump` doctor field (`sprint-AQ2-7-herdr-queue-wake.md`
  deliverable 5) surfaces this same shared state, not a second breaker.
- Bounds are named constants (`HERDR_BACKOFF_BASE_MS = 1_000`,
  `HERDR_BACKOFF_CAP_MS = 30_000`); no config surface in Phase AQ.
- Required evidence (AQ2.6 AC 11 extension, and AQ2.7 AC 5): with a fake
  adapter returning `server_not_running` three times — from `prompt` (AQ2.6)
  or from `list` (AQ2.7) — exactly three spawns occur across ≥ 7 s of
  wall-clock attempts and the fourth attempt after `retry_after` succeeds
  and closes the breaker. One breaker implementation, exercised from both
  call sites.
