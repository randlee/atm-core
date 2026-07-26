# AI.22 advertised-IP HTTPS self-send — independent quality review

**Author/executor:** an independent QA subagent dispatched by `quality-mgr`
(general-purpose agent instance, no prior involvement in commit `95dbb094` or
in authoring the original artifact below). Not `arch-ctm` (fix author) and not
a same-party self-review by `quality-mgr`. This addresses AI23-BLOCK-003
(the original artifact was same-commit/same-author evidence reviewed only by
the same party who closed the finding, which does not satisfy the sprint
doc's independent-execution requirement).

Run on 2026-07-26 against the running singleton daemon (PID 36925,
`/opt/homebrew/bin/atm-daemon`, `1.3.2-beta.22`), confirmed byte-identical for
`crates/` between `feature/pAI-s22-loopback-self-send-exemption@95dbb094` and
`integrate/phase-AI@24d14beb` via `git diff 95dbb094 24d14beb -- crates/`
(empty diff) — i.e. this is a genuine release build of the code under review,
not a rebuild-from-scratch substitute.

## Real-transport confirmation

- `lsof -iTCP -sTCP:LISTEN` showed PID 36925 bound to `192.168.128.82:43101`
  (real, non-loopback interface; confirmed via `ifconfig`).
- A plaintext HTTP probe (`nc`) against that address:port received a binary
  TLS-alert response and connection close, confirming a genuine rustls TLS
  listener terminates the socket — not an in-process/mock shortcut.

## The send

| Check | Result |
| --- | --- |
| `atm doctor --json` | `1.3.2-beta.22` CLI/daemon; healthy; 1 peer interface enabled; 5 trusted peers incl. `192.168.128.82` |
| `atm send loopback-peer@atm-dev.192.168.128.82 ... --json` | `"outcome":"sent"` |
| Message ID | `01KYEA7C6DQZ59ZPEPSN6EFVB2` |
| `atm read --message-id ... --json` | same ULID; `peerOutbound.host` = `192.168.128.82`, survived into the canonical `WriteRequest.to.host` |
| Peer delivery log (`/Users/randlee/.atm/logs/atm.log.jsonl`, filtered on this message_id) | `action:"send" outcome:"sent"` → `action:"peer_delivery" outcome:"write_persisted" fields:{peer:"192.168.128.82"}` → `action:"peer_delivery" outcome:"peer_delivery_confirmed" fields:{peer:"192.168.128.82"}` |
| `atm doctor` peer-link projection | `192.168.128.82` flipped `misconfigured` → `quality:"healthy"`, `last_success_at` matches `peer_delivery_confirmed` timestamp to the millisecond |

Used the pre-provisioned `loopback-peer@atm-dev` roster identity (set up for
this exemption) as recipient, to avoid polluting real agents' mailboxes.

## Component naming

The codebase has no `#[instrument]`/named tracing spans on these internal
functions (confirmed by grep across `crates/atm-daemon` and
`crates/atm-core/src/send` — the only named runtime log events are the two
`peer_delivery` lines above). Naming is therefore by file:line citation, with
the single-call-path structure of the source making execution provable rather
than assumed for this exact message_id:

- **`ApiRouter::route`** — `crates/atm-daemon/src/runtime_health.rs:906-949` (validates ingress, then calls `dispatch_with_deadline`)
- **Dispatcher** — `DaemonRequestDispatcher::dispatch_with_deadline` (`runtime_health.rs:512-521`) → `route_write` (`runtime_health.rs:523-543`)
- **Persistence** — `MessageWriter::write` (`runtime_health.rs:588-598`) → `persist_local_write` (`runtime_health.rs:545-547`) → `prepare_write_with_runtime` (`crates/atm-core/src/send/mod.rs:466`)
- **`PostWriteRouter::dispatch`** — `crates/atm-daemon/src/runtime_health/peer_delivery_router.rs:14-78` — the sole call site (`route_write` line 530) that can emit `WritePersisted`/`PeerDeliveryConfirmed`; those two log lines existing for this message_id is proof-by-construction that route → dispatcher → persistence → `PostWriteRouter::dispatch` all executed for this request.

## Verdict against sprint doc lines 140-142, 160-163

| Requirement | Result |
| --- | --- |
| Real CLI, real advertised-IP TCP+TLS listener, no mock/direct-dispatch | **PASS** |
| Router/dispatcher/persistence/PostWriteRouter named with evidence tying to this request | **PASS** (via call-graph citation + runtime log correlation; no source-level trace spans exist to quote directly — tracked separately, see AI23-IMPORTANT follow-up) |
| Independent party (not arch-ctm, not quality-mgr self-review) | **PASS** |

No files were edited/committed by the reviewing agent and no daemon
switch/restart was performed; the correct release build was already the live
singleton.
