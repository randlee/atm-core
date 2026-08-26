# Herdr CLI Contract Fixture — atm-core Phase AQ (AQ2.6 / AQ2.7)

**Provenance: derived-from-source, not live-captured.** Every transcript below
is reconstructed from the Herdr source at `d79fd746` (identical to tag
`v0.8.2` for all cited surfaces) against the installed binary `herdr 0.8.2`
(wire protocol 20). No live Herdr agent was prompted, waited on, started, or
renamed to produce this file. AQ2.6's required live validation replaces these
rows with captured output; until then fixture tests must assert on
`error.code`, `result.type`, `result.agent.agent_status`, stream (stdout vs
stderr), and exit code — never on `message` text or key order.

Conventions:

- `argv` is the literal `execve` vector; no shell.
- `<sock>` = `~/.config/herdr/herdr.sock` unless `HERDR_SOCKET_PATH` or
  `HERDR_SESSION` is set in the daemon environment (`src/session.rs:173-180`).
- `AgentInfo` bodies are abbreviated `{…}`; fields present on every success:
  `terminal_id, name, agent, agent_status, workspace_id, tab_id, pane_id,
  focused, state_change_seq, revision` (`src/api/schema/agents.rs:184-223`).
- Placeholder member name: `arch-ctm` (valid under `^[a-z][a-z0-9_-]{0,31}$`).

---

## F1. Immediate steer — `agent prompt` (AQ2.6 `HerdrReceivedHook`)

argv:

```text
["herdr","agent","prompt","arch-ctm","You have unread ATM messages. Run: atm read"]
```

Request id: `cli:agent:prompt` (`src/cli/agent.rs:833`).

### F1.1 success (agent idle or working)

| stream | content |
| --- | --- |
| stdout | `{"id":"cli:agent:prompt","result":{"type":"agent_prompted","agent":{"terminal_id":"…","name":"arch-ctm","agent":"codex","agent_status":"working","workspace_id":"…","tab_id":"…:t1","pane_id":"…:p1","focused":false,"state_change_seq":42,"revision":…}}}` |
| stderr | (empty) |
| exit | `0` |

Source: `src/app/api/agents.rs:121-130` (text written, Enter scheduled +300 ms), `src/cli.rs:744-745`.
Note: `agent_status` in the response is the status *at submission*; it may still read `idle`.

### F1.2 `agent_blocked` (agent at approval/question UI) — **no input written**

| stream | content |
| --- | --- |
| stdout | (empty) |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"agent_blocked","message":"agent arch-ctm is blocked and requires interactive input"}}` |
| exit | `1` |

Source: `src/app/api/agents.rs:82-91` (check precedes `try_send_bytes` at `:123`), `src/cli.rs:739-742`.

### F1.3 `agent_not_found` (no agent named `arch-ctm` on this server)

| stream | content |
| --- | --- |
| stdout | (empty) |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"agent_not_found","message":"agent target arch-ctm not found"}}` |
| exit | `1` |

Source: `src/app/terminal_targets.rs:103-105`, `src/app/agents.rs:290-293`.
Variant (pane exists but terminal/runtime gone): same code, same message, `src/app/api/agents.rs:299-305`.

### F1.4 `agent_target_ambiguous` (two agents resolve to the same target)

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"agent_target_ambiguous","message":"agent target arch-ctm is ambiguous; candidates: terminal_id=… pane_id=…:p1 workspace_id=… tab_id=…:t1 cwd=/… status=Idle; terminal_id=… …"}}` |
| exit | `1` |

Source: `src/app/agents.rs:294-318`. Reachable only if name uniqueness is bypassed (e.g. session restore of stale names); atm-core treats as advisory failure.

### F1.5 `agent_not_ready` (agent still launching, or no longer the pane foreground process)

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:prompt","error":{"code":"agent_not_ready","message":"agent arch-ctm is not an active named agent"}}` or `…"message":"agent arch-ctm is no longer the pane foreground process"}}` |
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
| `["herdr","agent","prompt","arch-ctm"]` | `agent prompt requires text` | `2` |
| `["herdr","agent","prompt","arch-ctm","x","--timeout","1000"]` | `--timeout requires --wait` | `2` |
| `["herdr","agent","prompt","arch-ctm","x","--until","idle"]` | `--until requires --wait` | `2` |
| `["herdr","agent","prompt","arch-ctm",""]` | `{"id":"cli:agent:prompt","error":{"code":"empty_agent_prompt","message":"agent prompt must not be empty"}}` | `1` |

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

## F2. Queue wake gate — `agent wait` (AQ2.7 `HerdrQueueWakePump`)

argv (example bound: 45 min = 2 700 000 ms):

```text
["herdr","agent","wait","arch-ctm","--until","idle","--until","done","--until","blocked","--timeout","2700000"]
```

Request id: `cli:agent:wait` (`src/cli/agent.rs:553`).

### F2.1 success — agent reached (or already was) `idle`

| stream | content |
| --- | --- |
| stdout | `{"id":"cli:agent:wait","result":{"type":"agent_info","agent":{"terminal_id":"…","name":"arch-ctm","agent":"codex","agent_status":"idle",…}}}` |
| exit | `0` |

Source: `src/api/wait.rs:150-152` (immediate), `:465-467`/`:489-491` (observed), `:600-609`.
`agent_status` may equally be `"done"` → pump proceeds; `"blocked"` → see F2.2.

### F2.2 success with `agent_status: "blocked"` — gate holds, **no prompt**

| stream | content |
| --- | --- |
| stdout | `{"id":"cli:agent:wait","result":{"type":"agent_info","agent":{…,"agent_status":"blocked",…}}}` |
| exit | `0` |

Note: `blocked` is a *matched* state, so this is exit 0 on stdout, not an error. The pump must switch on `result.agent.agent_status`, not on exit code (AQ2.7 `held_blocked`).

### F2.3 timeout (agent stayed `working`/`unknown` for the whole bound)

| stream | content |
| --- | --- |
| stdout | (empty) |
| stderr | `{"id":"cli:agent:wait","error":{"code":"timeout","message":"timed out waiting for agent status"}}` |
| exit | `1` |

Source: `src/api/wait.rs:470-495`, `:616-619`. Deadline check re-probes once; a match exactly at deadline still returns F2.1.

### F2.4 `agent_not_found` at start (no live agent named `arch-ctm`)

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:wait","error":{"code":"agent_not_found","message":"agent target arch-ctm not found"}}` |
| exit | `1` |

Source: `src/api/wait.rs:141-148` → `agent_get` → `src/app/agents.rs:290-293`. AQ2.7 outcome `held_target_not_present`.

### F2.5 `agent_not_running` (agent exited / pane closed / renamed away during the wait)

| stream | content |
| --- | --- |
| stderr | `{"id":"cli:agent:wait","error":{"code":"agent_not_running","message":"agent is no longer running in the target pane"}}` |
| exit | `1` |

Source: `src/api/wait.rs:396-436`, `:450-459`, `:643-659`. AQ2.7 treats as `held_target_not_present`.

### F2.6 server down

Identical to F1.6 with `"id":"cli:agent:wait"`. exit `1`.

### F2.7 cancellation (pump shutdown / daemon deadline)

atm-core sends SIGTERM then SIGKILL to the `herdr` child. No output is
produced by the client; the server notices the closed socket within 100 ms
(`src/api/wait.rs:371`, `src/api/server.rs:782-791`) and abandons the wait.
No input is written by `wait` under any outcome. Fixture asserts: child
reaped, no stdout, no subsequent prompt.

### F2.8 argv construction bug (must be unreachable)

| argv | stderr | exit |
| --- | --- | --- |
| `[…,"--until","ready"]` | `invalid agent status: ready (expected idle, working, blocked, done, or unknown)` (plain text, `src/cli.rs:897-908`) | `2` |
| `[…,"--timeout","abc"]` | `invalid value for --timeout: abc` | `2` |
| `[…,"--workspace","atm-dev"]` | `unknown option: --workspace` | `2` |

---

## F3. Launch convention only (never emitted by the daemon)

### F3.1 `agent start`

```text
["herdr","agent","start","arch-ctm","--kind","codex","--pane","<workspace_id>:p1"]
```

| outcome | stream / content | exit |
| --- | --- | --- |
| success | stdout `{"id":"cli:agent:start","result":{"type":"agent_started","agent":{…,"name":"arch-ctm","agent":"codex","agent_status":"idle","interactive_ready":true,…},"argv":["codex"]}}` | 0 |
| name in use | stderr `{"id":"cli:agent:start","error":{"code":"agent_name_taken","message":"agent name arch-ctm is already used; candidates: …"}}` | 1 |
| bad name (`Arch-CTM`) | stderr `{…"code":"invalid_agent_name","message":"agent name must start with a lowercase letter and contain only lowercase letters, digits, '-' or '_' (1-32 characters)"}` | 1 |
| pane not a shell | stderr `{…"code":"agent_pane_busy",…}` (after ≤2 s shell-readiness retry, `src/cli/agent.rs:355-404`) | 1 |
| pane missing | stderr `{…"code":"agent_pane_not_found",…}` | 1 |
| startup timeout | stderr `{"id":"cli:agent:start","error":{"code":"timeout","message":"timed out waiting for agent startup"}}` | 1 |
| missing `--pane` | stderr `missing required --pane` (plain) | 2 |

Source: `src/cli/agent.rs:289-436`, `:562-632`, `src/app/agents.rs:145-227`, `:229-289`.

### F3.2 `agent rename`

```text
["herdr","agent","rename","<workspace_id>:p1","arch-ctm"]
```

| outcome | stderr / stdout | exit |
| --- | --- | --- |
| success | stdout `{"id":"cli:agent:rename","result":{"type":"agent_info","agent":{…,"name":"arch-ctm",…}}}` | 0 |
| target not an agent | stderr `{…"code":"agent_not_found","message":"agent target does not currently host an agent"}` | 1 |
| name taken elsewhere | stderr `{…"code":"agent_name_taken",…}` | 1 |
| invalid name | stderr `{…"code":"invalid_agent_name",…}` | 1 |
| launch pending | stderr `{…"code":"agent_launch_pending","message":"agent name cannot change while startup is pending"}` | 1 |

Source: `src/cli/agent.rs:751-769`, `src/app/agents.rs:90-143`, `:320-360`.

---

## F4. Read-only commands actually run for this fixture (live, 2026-08-26)

```text
$ herdr --version
herdr 0.8.2
$ herdr status
client:  version: 0.8.2  channel: stable  protocol: 20
server:  status: running  version: 0.8.2  protocol: 20  compatible: yes
         socket: /Users/randlee/.config/herdr/herdr.sock
update:  restart_needed: no
(exit 0)
$ herdr agent prompt --help    # confirms: TARGET, TEXT, --wait, --until, --timeout; 5000ms agent_prompt_stalled note
$ herdr agent wait --help      # confirms: default idle/done/blocked; --until unknown explicit; no --timeout = indefinite
$ herdr agent start --help     # confirms: --kind required, --pane required; kinds include claude, codex, hermes
$ herdr agent rename --help    # confirms: <TARGET> <NAME>|--clear
```

No `herdr agent prompt/wait/start/rename` was executed against a target.
