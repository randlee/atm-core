# Herdr CLI Contract Fixture — atm-core Phase AQ (AQ2.6 / AQ2.7)

**Provenance: derived-from-source, not live-captured.** Every transcript below
is reconstructed from the Herdr source at `d79fd746` (identical to tag
`v0.8.2` for all cited surfaces) against the installed binary `herdr 0.8.2`
(wire protocol 20). No live Herdr agent was prompted, listed, waited on,
started, or renamed to produce this file. AQ2.6's required live validation
replaces these rows with captured output; until then fixture tests must
assert on `error.code`, `result.type`, `result.agent.agent_status` (or, for
`agent list`, each entry's `agent_status` and `name`), stream (stdout vs
stderr), and exit code — never on `message` text or key order. F2's primary
rows (`agent list`) are what AQ2.7's rewritten poll-based pump actually
exercises; F2's reference-only `agent wait` rows document ADR-058 D2's
retained-but-unemitted contract and are not asserted against by any Phase AQ
fixture test.

Conventions:

- `argv` is the literal `execve` vector; no shell.
- **Environment (per ADR-058 D1, one model only).** `HERDR_SESSION` is set
  on the `herdr` **child process, per invocation**, only when the emitting
  member's roster row carries `LocalMessageReceivedBackend::Herdr { session:
  Some(_) }` — the value comes from that per-member roster field (AQ1's
  `HerdrSession`, `crates/atm-core/src/delivery_channel.rs`), never from the
  atm daemon's own process environment, which is never consulted. When the
  member's `session` is `None`, `HERDR_SESSION` is left unset on the child
  and `<sock>` resolves to Herdr's default `~/.config/herdr/herdr.sock`
  (`src/session.rs:96-101`, `:161-171`, `:173-180`). `HERDR_SOCKET_PATH`, if
  present in the daemon's own environment, is inherited unchanged (it is not
  member-specific and this ADR does not change that).
- `AgentInfo` bodies are abbreviated `{…}`. Fields marked required in the
  schema and therefore present on every success: `terminal_id, agent_status,
  workspace_id, tab_id, pane_id, focused, state_change_seq, revision`. `name`
  and `agent` are `Option<String>` with `skip_serializing_if =
  "Option::is_none"` and are **omitted**, not `null`, for an unnamed or
  undetected agent (`src/api/schema/agents.rs:184-223`); the transcripts
  below show them only because the fixture's placeholder agent is named and
  detected.
- Placeholder member name: `agent-a` (valid under `^[a-z][a-z0-9_-]{0,31}$`,
  neutral — not a real ATM team member). Placeholder Herdr session name where
  one is shown: `session-a`. Placeholder team name: `team-x`. Placeholder
  pane/workspace id: `ws1:p1`.

---

## F1. Immediate steer — `agent prompt` (AQ2.6 `HerdrReceivedHook`)

argv:

```text
["herdr","agent","prompt","agent-a","<rendered built-in nudge template>"]
```

The fourth element is the rendered built-in nudge template supplied by
atm-core; it is passed byte-for-byte, including embedded newlines.

Request id: `cli:agent:prompt` (`src/cli/agent.rs:833`).

### F1.1 success (agent idle or working)

| stream | content |
| --- | --- |
| stdout | `{"id":"cli:agent:prompt","result":{"type":"agent_prompted","agent":{"terminal_id":"…","name":"agent-a","agent":"codex","agent_status":"working","workspace_id":"…","tab_id":"…:t1","pane_id":"…:p1","focused":false,"state_change_seq":42,"revision":…}}}` |
| stderr | (empty) |
| exit | `0` |

Source: `src/app/api/agents.rs:121-130` (text written, Enter scheduled +300 ms), `src/cli.rs:744-745`.
Note: `agent_status` in the response is the status *at submission*; it may still read `idle`.

### F1.2 `agent_blocked` (agent at approval/question UI) — **no input written**

| stream | content |
| --- | --- |
| stdout | (empty) |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"agent_blocked","message":"agent agent-a is blocked and requires interactive input"}}` |
| exit | `1` |

Source: `src/app/api/agents.rs:82-91` (check precedes `try_send_bytes` at `:123`), `src/cli.rs:739-742`.

### F1.3 `agent_not_found` (no agent named `agent-a` on this server)

| stream | content |
| --- | --- |
| stdout | (empty) |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"agent_not_found","message":"agent target agent-a not found"}}` |
| exit | `1` |

Source: `src/app/terminal_targets.rs:103-105`, `src/app/agents.rs:290-293`.
Variant (pane exists but terminal/runtime gone): same code, same message, `src/app/api/agents.rs:299-305`.

### F1.4 `agent_target_ambiguous` (two agents resolve to the same target)

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"agent_target_ambiguous","message":"agent target agent-a is ambiguous; candidates: terminal_id=… pane_id=…:p1 workspace_id=… tab_id=…:t1 cwd=/… status=Idle; terminal_id=… …"}}` |
| exit | `1` |

Source: `src/app/agents.rs:294-318`. Reachable only if name uniqueness is bypassed (e.g. session restore of stale names); atm-core treats as advisory failure.

### F1.5 `agent_not_ready` (agent still launching, or no longer the pane foreground process)

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"agent_not_ready","message":"agent agent-a is not an active named agent"}}` or `…"message":"agent agent-a is no longer the pane foreground process"}}` |
| exit | `1` |

Source: `src/app/api/agents.rs:92-97`, `:101-110`, `:291-297`.

### F1.6 server down (`server_not_running`)

| stream | content |
| --- | --- |
| stdout | (empty) |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"server_not_running","message":"no herdr server is running at /Users/<u>/.config/herdr/herdr.sock; run `herdr` to start or attach it"}}` |
| exit | `1` |

Source: `src/cli.rs:762-768` (ping fails on connect), `:819-844`, `src/cli/server_not_running.rs:28-40`, `src/main.rs:572-578`.
With `HERDR_SESSION=<name>` the message ends `run `herdr session attach <name>` …` (`src/session.rs:103-108`).

### F1.7 protocol skew (`protocol_mismatch`)

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"protocol_mismatch","message":"client protocol 21 is newer than server protocol 20; restart the Herdr server before using this command. Stop the old server to use the new version.\nStopping exits pane processes.\nRun `herdr server stop`, then run `herdr` again."}}` |
| exit | `1` |

Source: `src/cli/protocol_guard.rs:16-43`, `src/cli.rs:777-797`, `src/main.rs:571`. This is exactly what a binary built from `d79fd746` would print against the currently running 0.8.2 server.

### F1.8 argv construction bug (must be unreachable from atm-core)

| argv | stderr (plain text) | exit |
| --- | --- | --- |
| `["herdr","agent","prompt","agent-a"]` | `agent prompt requires text` | `2` |
| `["herdr","agent","prompt","agent-a","x","--timeout","1000"]` | `--timeout requires --wait` | `2` |
| `["herdr","agent","prompt","agent-a","x","--until","idle"]` | `--until requires --wait` | `2` |
| `["herdr","agent","prompt","agent-a",""]` | `{"id":"cli:agent:prompt","error":{"code":"empty_agent_prompt","message":"agent prompt must not be empty"}}` | `1` |

Source: `src/cli/agent.rs:778-781`, `:824-831`, `src/app/api/agents.rs:63-65`.

### F1.9 timeout

Not applicable: without `--wait` the request returns as soon as the server
schedules the write (`src/api/wait.rs:185-194`, dispatched with no app
timeout). The only time-bound failure is the atm-core `RequestDeadline`
killing the child (AQ2.6 deliverable 3). Server-side `server_unavailable`
(`{"code":"server_unavailable","message":"server is shutting down"}` or
`"failed to dispatch request: …"`, `src/api/server.rs:371-377`, `:843-847`)
is the closest Herdr-emitted equivalent; exit 1.

---

## F2. Queue wake gate — `agent list` polling (AQ2.7 `HerdrQueueWakePump`)

**Rewritten 2026-08-26 (Rand): the pump polls, it does not `wait`.** Every
tick, AQ2.7 calls `agent list` once per distinct Herdr session and prompts
whichever pending members it observes `idle`/`done`. The `agent wait`
fixtures that governed the prior per-member-wait design are retained
below, relabeled reference-only (ADR-058 D2: documented, not emitted by
atm-core in Phase AQ) — nothing in this sprint's fixture tests exercises
them.

argv:

```text
["herdr","agent","list"]
```

Request id: `cli:agent:list` (`src/cli/agent.rs:445`). No target, no
`--session`/`--workspace` argument — session selection is env-mediated only
(Conventions above), identically to every other `herdr agent` subcommand.

### F2.1 success — multiple agents, mixed status, one session

| stream | content |
| --- | --- |
| stdout | `{"id":"cli:agent:list","result":{"type":"agent_list","agents":[{"terminal_id":"…","name":"agent-a","agent":"codex","agent_status":"idle","workspace_id":"…","tab_id":"…:t1","pane_id":"…:p1","focused":false,"state_change_seq":42,"revision":…},{"terminal_id":"…","name":"agent-b","agent":"claude","agent_status":"working","workspace_id":"…","tab_id":"…:t2","pane_id":"…:p2","focused":true,"state_change_seq":7,"revision":…},{"terminal_id":"…","agent_status":"idle","workspace_id":"…","tab_id":"…:t3","pane_id":"…:p3","focused":false,"state_change_seq":1,"revision":…}]}}` |
| stderr | (empty) |
| exit | `0` |

Source: `src/app/api/agents.rs:16-22` (`handle_agent_list` →
`collect_agent_infos`), `src/app/agents.rs:22-35`, `src/cli.rs:744-745`.
Note the third entry has no `name` (`Option<String>`, omitted not `null`,
`src/api/schema/agents.rs:187`) — an unnamed/undetected agent the pump can
never match to a member; `agent-a` is `idle` (pump claims/prompts),
`agent-b` is `working` (pump makes no claim this tick).

### F2.2 success — target member absent from the list (no error)

| stream | content |
| --- | --- |
| stdout | `{"id":"cli:agent:list","result":{"type":"agent_list","agents":[]}}` (or a non-empty array containing no entry named `agent-a`) |
| exit | `0` |

There is no `agent_not_found` here — `list` has no target to fail to
resolve. AQ2.7 treats a pending member's absence from `agents[]` as
`held_target_not_present`, pre-claim: no claim is taken, the marker is
retained, and the member is re-evaluated on the next tick
(ADR-058 D8's new absent-from-list row, D9.1).

### F2.3 server down

Identical to F1.6 with `"id":"cli:agent:list"`. exit `1`. Triggers the
D10.1 breaker exactly as a `prompt` failure does — one breaker, shared.

### F2.4 protocol skew

Identical to F1.7 with `"id":"cli:agent:list"`. exit `1`. Also a D10.1
breaker trigger.

### F2.5 argv construction bug (must be unreachable)

| argv | stderr (plain text) | exit |
| --- | --- | --- |
| `["herdr","agent","list","extra"]` | `usage: herdr agent list` | `2` |

Source: `src/cli/agent.rs:439-442`.

### F2.6 child-process bound (ADR-058 D10)

`agent list` has no `--wait`/`--timeout` of its own; the pump's call site is
bound solely by atm-core's own external deadline around the child's
wait-for-exit — the same 5 s steer/list bound `prompt` uses (D10), not a
per-member wait bound (removed). A fixture test doubles
`HerdrProcessAdapter::list` with a future that never resolves and asserts
the call returns `HerdrTimedOut`, the child is killed, and the D10.1
breaker opens.

### F2.7 session grouping (two sessions, two children)

Two eligible Herdr members configured with `session-a` and `session-b`
respectively produce two `agent list` invocations in one tick:

```text
["herdr","agent","list"]   # HERDR_SESSION=session-a
["herdr","agent","list"]   # HERDR_SESSION=session-b
```

A member with no configured session produces a third bucket with
`HERDR_SESSION` unset (Herdr's default server). The fixture asserts exactly
one `list` child per distinct session value present among that tick's
eligible members — never one per member.

---

### Reference only — `agent wait` (ADR-058 D2: documented, not emitted by atm-core in Phase AQ)

The rows below describe the prior per-member `agent wait` design
(superseded 2026-08-26). `HerdrProcessAdapter::wait` stays defined
(`atm-herdr`) and this remains its accurate contract — retained here so a
future phase reviving a lifecycle-blocking gate has a ready fixture — but
no Phase AQ sprint's tests construct or assert against this argv.

argv (example bound: 45 min = 2 700 000 ms):

```text
["herdr","agent","wait","agent-a","--until","idle","--until","done","--until","blocked","--timeout","2700000"]
```

Request id: `cli:agent:wait` (`src/cli/agent.rs:553`).

**Reference: success — agent reached (or already was) `idle`**

| stream | content |
| --- | --- |
| stdout | `{"id":"cli:agent:wait","result":{"type":"agent_info","agent":{"terminal_id":"…","name":"agent-a","agent":"codex","agent_status":"idle",…}}}` |
| exit | `0` |

Source: `src/api/wait.rs:150-152` (immediate), `:465-467`/`:489-491` (observed), `:600-609`.
`agent_status` may equally be `"done"` → pump proceeds; `"blocked"` → see below.

**Reference: success with `agent_status: "blocked"` — gate holds, no prompt**

| stream | content |
| --- | --- |
| stdout | `{"id":"cli:agent:wait","result":{"type":"agent_info","agent":{…,"agent_status":"blocked",…}}}` |
| exit | `0` |

Note: `blocked` is a *matched* state, so this is exit 0 on stdout, not an error.

**Reference: timeout (agent stayed `working`/`unknown` for the whole bound)**

| stream | content |
| --- | --- |
| stdout | (empty) |
| stderr | `{"id":"cli:agent:wait","error":{"code":"timeout","message":"timed out waiting for agent status"}}` |
| exit | `1` |

Source: `src/api/wait.rs:470-495`, `:616-619`. Deadline check re-probes once; a match exactly at deadline still returns the success row above.

**Reference: `agent_not_found` at start (no live agent named `agent-a`)**

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:wait","error":{"code":"agent_not_found","message":"agent target agent-a not found"}}` |
| exit | `1` |

Source: `src/api/wait.rs:141-148` → `agent_get` → `src/app/agents.rs:290-293`.

**Reference: `agent_not_running` (agent exited / pane closed / renamed away during the wait)**

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:wait","error":{"code":"agent_not_running","message":"agent is no longer running in the target pane"}}` |
| exit | `1` |

Source: `src/api/wait.rs:396-436`, `:450-459`, `:643-659`.

**Reference: server down**

Identical to F1.6 with `"id":"cli:agent:wait"`. exit `1`.

**Reference: cancellation (would-be pump shutdown / daemon deadline)**

atm-core sends SIGTERM then SIGKILL to the `herdr` child. No output is
produced by the client; the server notices the closed socket within 100 ms
(`src/api/wait.rs:371`, `src/api/server.rs:782-791`) and abandons the wait.
No input is written by `wait` under any outcome.

**Reference: argv construction bug (must be unreachable)**

| argv | stderr | exit |
| --- | --- | --- |
| `[…,"--until","ready"]` | `invalid agent status: ready (expected idle, working, blocked, done, or unknown)` (plain text, `src/cli.rs:897-908`) | `2` |
| `[…,"--timeout","abc"]` | `invalid value for --timeout: abc` | `2` |
| `[…,"--workspace","team-x"]` | `unknown option: --workspace` | `2` |

---

## F3. Doctor probe — `agent get` (AQ2.6 `atm doctor`, ADR-058 D9)

argv:

```text
["herdr","agent","get","agent-a"]
```

Request id: `cli:agent:get` (`src/cli/agent.rs:459-465`). Same target
resolution as F1/F2 (`App::resolve_agent_target`); no PTY write, no
foreground check — read-only.

### F3.1 success

| stream | content |
| --- | --- |
| stdout | `{"id":"cli:agent:get","result":{"type":"agent_info","agent":{"terminal_id":"…","name":"agent-a","agent":"codex","agent_status":"idle","workspace_id":"…","tab_id":"…:t1","pane_id":"ws1:p1","focused":false,"state_change_seq":42,"revision":…}}}` |
| stderr | (empty) |
| exit | `0` |

Source: `src/app/api/agents.rs:25-32` (`handle_agent_get`), `src/cli.rs:738-746`.
`atm doctor` reads only `result.agent.agent_status`; all other `AgentInfo`
fields are advisory-only, per ADR-058 D9 / "Explicitly NOT relied upon".

### F3.2 `agent_not_found` — reported by `atm doctor` as "agent not visible
in the member's configured Herdr session"

| stream | content |
| --- | --- |
| stdout | (empty) |
| stderr | `{"id":"cli:agent:get","error":{"code":"agent_not_found","message":"agent target agent-a not found"}}` |
| exit | `1` |

Source: `src/app/agents.rs:64-73`, `:288-296`. Emitted when the member's
stored `HerdrSession` (or lack of one) does not match the Herdr session the
agent actually runs in — the child is still spawned with
`HERDR_SESSION=<member.session>` per the Conventions above, only unset when
the member has none.

### F3.3 `agent_target_ambiguous`

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:get","error":{"code":"agent_target_ambiguous","message":"agent target agent-a is ambiguous; candidates: terminal_id=… pane_id=ws1:p1 workspace_id=… tab_id=…:t1 cwd=/… status=Idle; terminal_id=… …"}}` |
| exit | `1` |

Source: `src/app/agents.rs:297-317`.

### F3.4 server down / protocol skew

Identical to F1.6/F1.7 with `"id":"cli:agent:get"`. exit `1`.

### F3.5 argv construction bug (must be unreachable from atm-core)

| argv | stderr (plain text) | exit |
| --- | --- | --- |
| `["herdr","agent","get"]` | `usage: herdr agent get <target>` | `2` |
| `["herdr","agent","get","agent-a","extra"]` | `usage: herdr agent get <target>` | `2` |

Source: `src/cli/agent.rs:450-457`.

### F3.6 child-process bound (ADR-058 D10)

`agent get` has no `--wait`/`--timeout` of its own; the doctor call site is
bound solely by atm-core's own external deadline around the child's
wait-for-exit, independent of anything Herdr-side. A fixture test doubles
`HerdrProcessAdapter::get` with a future that never resolves and asserts the
call returns `HerdrTimedOut` and the child is killed, mirroring F1's steer
bound (5 s) rather than F2's per-member wait bound, since a doctor probe is
a synchronous, bounded read like `prompt`, not a long-lived wait.

---

## F6. Lead escalation notification (AX6)

AX6 uses the public `HerdrProcessAdapter::notify` operation for lead and
blocked-task escalation. The exact argv is:

```text
["herdr","notification","show","Task escalation","--body","task t-42 has been reminded 10 times","--sound","request"]
```

The title and body are each one argv element, including any newlines in the
body. There is no pane target, session argument, tmux operation, or shell
interpolation. A successful command exits `0`; a non-zero exit is a typed
adapter failure and does not change the independent mail-write results.

## F4. Launch convention only (never emitted by the daemon)

### F4.1 `agent start`

```text
["herdr","agent","start","agent-a","--kind","codex","--pane","ws1:p1"]
```

| outcome | stream / content | exit |
| --- | --- | --- |
| success | stdout `{"id":"cli:agent:start","result":{"type":"agent_started","agent":{…,"name":"agent-a","agent":"codex","agent_status":"idle","interactive_ready":true,…},"argv":["codex"]}}` | 0 |
| name in use | stderr `{"id":"cli:agent:start","error":{"code":"agent_name_taken","message":"agent name agent-a is already used; candidates: …"}}` | 1 |
| bad name (`Agent-A`) | stderr `{…"code":"invalid_agent_name","message":"agent name must start with a lowercase letter and contain only lowercase letters, digits, '-' or '_' (1-32 characters)"}` | 1 |
| pane not a shell | stderr `{…"code":"agent_pane_busy",…}` (after ≤2 s shell-readiness retry, `src/cli/agent.rs:355-404`) | 1 |
| pane missing | stderr `{…"code":"agent_pane_not_found",…}` | 1 |
| startup timeout | stderr `{"id":"cli:agent:start","error":{"code":"timeout","message":"timed out waiting for agent startup"}}` | 1 |
| missing `--pane` | stderr `missing required --pane` (plain) | 2 |

Source: `src/cli/agent.rs:289-436`, `:562-632`, `src/app/agents.rs:145-227`, `:229-289`.

### F4.2 `agent rename`

```text
["herdr","agent","rename","ws1:p1","agent-a"]
```

| outcome | stderr / stdout | exit |
| --- | --- | --- |
| success | stdout `{"id":"cli:agent:rename","result":{"type":"agent_info","agent":{…,"name":"agent-a",…}}}` | 0 |
| target not an agent | stderr `{…"code":"agent_not_found","message":"agent target does not currently host an agent"}` | 1 |
| name taken elsewhere | stderr `{…"code":"agent_name_taken",…}` | 1 |
| invalid name | stderr `{…"code":"invalid_agent_name",…}` | 1 |
| launch pending | stderr `{…"code":"agent_launch_pending","message":"agent name cannot change while startup is pending"}` | 1 |

Source: `src/cli/agent.rs:751-769`, `src/app/agents.rs:90-143`, `:320-360`.

---

## F5. Read-only commands actually run for this fixture (live, 2026-08-26)

```text
$ herdr --version
herdr 0.8.2
$ herdr status
client:  version: 0.8.2  channel: stable  protocol: 20
server:  status: running  version: 0.8.2  protocol: 20  compatible: yes
         socket: /Users/<u>/.config/herdr/herdr.sock
update:  restart_needed: no
(exit 0)
$ herdr agent prompt --help    # confirms: TARGET, TEXT, --wait, --until, --timeout; 5000ms agent_prompt_stalled note
$ herdr agent list --help      # confirms: no arguments; lists every detected agent on the connected server
$ herdr agent wait --help      # (reference only, D2) confirms: default idle/done/blocked; --until unknown explicit; no --timeout = indefinite
$ herdr agent get --help       # confirms: "Show an agent", usage: herdr agent get <target>
$ herdr agent start --help     # confirms: --kind required, --pane required; kinds include claude, codex, hermes
$ herdr agent rename --help    # confirms: <TARGET> <NAME>|--clear
```

No `herdr agent prompt/list/wait/get/start/rename` was executed against a
target.
