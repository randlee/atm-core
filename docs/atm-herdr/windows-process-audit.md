# Herdr Cross-Platform Process Audit

Status: AY.1 documentation baseline, 2026-09-06

This audit records the six Herdr operations used by ATM and the process and
endpoint behaviours that affect them. The source citations below are the
recorded citations from the Phase AY plan and ADR-058. The maintained Herdr
checkout named by the plan (`/Users/randlee/Documents/github/herdr`) was not
present in the AY.1 worktree, so this document does not claim a fresh checkout
or live observation. The recorded revisions are v0.8.0 (`857196de`), v0.8.2
(`34ba52cc`), and master `3a822e81`.

`ATM` relies on argv shape, selected JSON fields, stable error codes, and exit
status. It does not rely on mutable message text, JSON key order, workspace
labels, or live Windows evidence. Every Windows observation is explicitly
owned by AY.7 or release readiness.

## Audited behaviours

| Behaviour | Expected behaviour | Herdr v0.8.0 source file/line | Herdr v0.8.2 source file/line | Herdr `3a822e81` source file/line | Observation and verdict | Windows observation |
| --- | --- | --- | --- | --- | --- | --- |
| `agent prompt` — immediate steer | Emit `herdr agent prompt <AgentName> <rendered nudge template>` as one direct argv vector. Success is `result.type=agent_prompted`; failures are structured `error.code` with exit 1; malformed argv is exit 2. No `--wait` is emitted by ATM. | `src/cli/spec.rs:294-357` (prompt/wait flags) | `src/cli/agent.rs:778-841`; `src/app/api/agents.rs:63-130` | `src/cli/agent.rs:424-545,757-826`; prompt argv unchanged | Same argv, JSON shape, exit codes, and error-code surface. v0.8.2 adds the pre-write `agent_blocked` rejection where v0.8.0 submitted and then waited. **Verdict: no action; record the v0.8.0 behavioural delta.** | `AY.7 / release readiness: no recorded Windows Herdr observation in this repository; process correctness is verified in the Windows CI lane and live proof is release readiness.` |
| `agent wait` — retained contract | Emit `herdr agent wait <AgentName> --until idle --until done --until blocked --timeout <ms>` when a future caller uses the retained adapter method. Success is `result.type=agent_info`; `timeout`, `agent_not_running`, and target errors are structured exit-1 outcomes. Phase AQ does not invoke it. | `src/cli/spec.rs:294-357` | `src/cli/agent.rs:506-560`; `src/api/wait.rs:141-152,470-495,616-619` | `src/api/wait.rs:177-320,516,662-664` | The argv contract is unchanged. Master changes the activity gate and can surface `timeout` where v0.8.2 surfaced `agent_prompt_stalled`. ATM keys on codes, never text. **Verdict: no action; additive compatibility handling.** | `AY.7 / release readiness: no recorded Windows Herdr observation; Windows process and cancellation coverage belongs to AY.7.` |
| `agent get` — doctor presence probe | Emit `herdr agent get <AgentName>`. Success is `result.type=agent_info`; ATM reads only `agent_status`. Target errors are `agent_not_found` or `agent_target_ambiguous`; no PTY write occurs. | `src/cli/agent.rs:438-465` (agent dispatch/get surface) | `src/cli/agent.rs:20,450-465`; `src/app/api/agents.rs:25-32` | `src/cli/agent.rs:424-545`; `src/app/api/agents.rs:25-32` unchanged | Shape and error set are unchanged. **Verdict: no action.** | `AY.7 / release readiness: no recorded Windows Herdr observation; doctor’s Windows endpoint result is release-readiness evidence.` |
| `agent list` — queue-pump status probe | Emit `herdr agent list` with no target or session argument. Success is `result.type=agent_list` with `agents[]`; `name` and `agent_status` are the only ATM-consumed fields. | `src/cli/agent.rs:438-448` (recorded tag surface) | `src/cli/agent.rs:19,438-448`; `src/app/api/agents.rs:16-22` | `src/cli/agent.rs:424-545`; list argv and response shape unchanged | Shape, exit codes, and error set are unchanged. An absent name is a normal empty/list result, not `agent_not_found`. **Verdict: no action.** | `AY.7 / release readiness: no recorded Windows Herdr observation; Windows command construction and transport process coverage belong to AY.7.` |
| `notification show` — lead escalation | Emit `herdr notification show <title> --body <body> --sound request`, preserving title and body as separate argv elements. Success is exit 0; non-zero is a typed adapter failure. | `src/cli/notification.rs:28` | `src/cli/notification.rs:28` | `src/cli/notification.rs` (unchanged surface; source citation recorded in the Phase AY plan) | The argv and failure contract are unchanged. **Verdict: no action.** | `AY.7 / release readiness: no recorded Windows Herdr observation; Windows process creation and console suppression are AY.7, live proof is release readiness.` |
| `status server --json` — doctor server status | Emit `herdr status server --json` for the CLI doctor path. Record JSON `version` and `protocol`; socket transport uses `ping` instead. This is doctor-only, never a nudge path. | `src/cli/status.rs` (verified at v0.8.0 revision `346411fa`) | `src/cli/status.rs` (verified at v0.8.2 revision `9eb52145`) | `src/cli/status.rs` (master; unchanged response fields) | `version` and `protocol` are present at all three references. **Verdict: no action; retain as a six-operation ledger row.** | `AY.7 / release readiness: no recorded Windows Herdr observation; endpoint and doctor proof are release-readiness evidence.` |
| Session and endpoint selection | Resolve explicit session, then `HERDR_SOCKET_PATH`, then `HERDR_SESSION`, then the default `<config_dir>/herdr.sock`. A named session is a separate Herdr server, not a workspace. ATM supplies the member session only on the child environment. | `src/session.rs` (recorded endpoint model; exact tag line not retained) | `src/session.rs:96-101,161-181`; `src/ipc.rs:44-51,137,247` | `src/session.rs:96-101,161-181`; `src/ipc.rs:44-51,137,247` byte-identical | The endpoint model is unchanged from v0.8.2 to master. The AY.1 ledger records v0.8.0 as the minimum compatibility release without inventing an unrecorded line citation. **Verdict: no action.** | `AY.7 / release readiness: named-pipe naming and per-user endpoint behaviour require Windows CI/process coverage; no live Windows evidence is claimed here.` |
| Transport, framing, and protocol | Use local sockets with one NDJSON request line and one JSON response line. Unix uses UDS; Windows uses a named pipe. `PROTOCOL_VERSION` is a secondary bincode-client fact, not the NDJSON compatibility floor. | `src/ipc.rs` and `src/protocol/wire.rs` (recorded v0.8.0 protocol 19) | `src/ipc.rs:10-11,137,247`; `src/protocol/wire.rs:20` (protocol 20) | `src/ipc.rs:10-11,137,247`; `src/protocol/wire.rs:20` → 22 | The transport and NDJSON shapes are unchanged; only the secondary protocol integer drifts 19 → 20 → 22. **Verdict: no action; govern minimum release with ADR-061.** | `AY.7 / release readiness: Windows named-pipe and kill/reap correctness are AY.7; no live pipe observation is recorded here.` |
| Error and exit-code mapping | Exit 0 is success JSON; exit 1 is structured failure JSON on stderr; exit 2 is usage/argv failure. ATM matches stable `error.code`, tolerates unknown fields/codes, and never matches `error.message`. | `src/cli.rs` / `src/app/api/agents.rs` (recorded tag sources) | `src/cli.rs:738-797`; `src/app/api/agents.rs:82-110,290-318` | `src/cli.rs:738-797`; `src/api/wait.rs:662-664` for changed wait text | v0.8.0 → v0.8.2: `agent_blocked` becomes a pre-write rejection; start-only codes are not ATM-path codes. v0.8.2 → master: `agent_prompt_stalled`/`timeout` wait outcome changes, while codes and JSON shapes remain compatible. **Verdict: no action; fake replay must cover both outcomes.** | `AY.7 / release readiness: no recorded Windows error transcript; CRLF decoding and process failure mapping are AY.7.` |
| Windows packaging and process lifecycle | A released Windows artifact exists from v0.8.2 onward; v0.8.0 had no official Windows release artifact. ATM must treat Windows as supported, resolve `herdr.exe` per call, suppress console creation, and kill/reap bounded children. | `distribution/` (v0.8.0 beta packaging; no release job) | `release.yml` / `windows-arm64.yml` (release artifact fact recorded by the plan) | `distribution/install.ps1:881-904`; `src/ipc.rs:342-345` | v0.8.2 is the first official Windows artifact; master changes PATH installation and retains the same API-pipe ACL limitation. **Verdict: AY.7 production fix/verification; not an AY.1 live-evidence claim.** | `AY.7 / release readiness: authoritative owner. This audit records no Windows pass, benchmark, or live machine result.` |

## Version drift summary

### v0.8.0 → v0.8.2

The six ATM operations retain the same argv, flags, JSON shapes, and exit
codes. The client-facing files changed, but the changes affecting ATM are
limited to the recorded `agent_blocked` behaviour: v0.8.2 rejects a prompt to
an already blocked agent before input, while v0.8.0 submitted and then waited.
ATM already has a typed `AgentBlocked` outcome, so the difference is handled
without a version branch. `agent_not_ready` and `agent_pane_busy` are
`agent start` codes and are not part of ATM's six-operation path.

The packaging difference is material but not a compatibility floor: v0.8.0
has no official Windows artifact, while v0.8.2 has a Windows x86_64 release
artifact. The minimum remains 0.8.0 on supported Unix platforms and the
Windows user population begins at the first official Windows artifact.

### v0.8.2 → `3a822e81`

The six operations retain their argv, flags, JSON shapes, exit codes, and
error-code set. The material behavioural drift is in `agent prompt --wait`:
the activity gate and the resulting `timeout`/`agent_prompt_stalled` outcome
changed. The protocol integer moved 20 → 22, but it versions Herdr's bincode
client socket, not the NDJSON API. Endpoint resolution, notification, and
agent response fields remain compatible. New JSON fields are additive and
must be ignored by ATM parsers.

Windows installer PATH handling changed in master, so ATM must resolve the
configured/binary alias per spawn and never persist a resolved executable
path. The Windows API pipe remains default-DACL; AY.7 records the boundary and
process implications.

## Verdict vocabulary

- **no action** — the recorded contract is compatible with ATM's one
  version-agnostic implementation.
- **production fix** — implementation or test work belongs to a later AY
  sprint, chiefly AY.7 for Windows process correctness.
- **upstream request** — Herdr must change before ATM can support the desired
  contract; no such request is raised by this audit.
