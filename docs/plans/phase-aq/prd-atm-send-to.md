# PRD Brief: ATM "Send To" Shell Integration

**Status:** Draft · **Owner:** Rand · **Date:** 2026-08-22
**Scope:** Phase 1 = `atm send`. Phase 2 = agent-assisted note drafting + non-ATM session launch. `atm queue` and `atm launch/spawn` remain follow-ons, but Phase 2 lays their groundwork.

---

## 1. Problem

Getting a file or folder from the OS file manager into a running agent's context requires: knowing which agent, knowing its host and cwd, copying the file somewhere it can read, and hand-typing an `atm send`. This is friction at exactly the moment a human wants to hand an artifact to a fleet member.

## 2. Goal

Right-click (Finder / Explorer) → pick one or more team members → the file(s) arrive in their inbox with a note, on any host. One gesture, no terminal.

## 2a. User Stories

**US-1 — Brief a new team member.** A planning agent has just been spawned onto a team. The human selects the plan and three background files in Finder, picks the new agent, and the drafter (forked from the briefing session) writes a kickoff note explaining what the files are and what's expected. Human edits one line, sends. *Exercises: multi-file, fork-session drafter, `note_source: drafted`.*

**US-2 — Fill a gap in a running agent's context.** A dev agent is stuck because a skill doc never made it into the repo. The human finds it in another repo, right-clicks, picks the agent, types "this is the convention you're missing — see §3". Sent in under ten seconds; no terminal. *Exercises: single file, existing member, human-authored note, same- or cross-host pull.*

**US-3 — Discuss a document, no team.** The human wants to think through a PDF with an agent that isn't on any ATM team. Right-click → "Open session" → Wyvern chat window opens with the file attached. No `atm send` at the end; the session *is* the destination. *Exercises: non-ATM path, Wyvern chat integration as a terminal stage.*

**US-4 — Chain.** US-3 produces a summary document. The human saves it, right-clicks it, sends it to a team member (US-2). Later that member's output gets sent on to a third. Every hop is the same gesture; nothing in the pipeline knows it's part of a chain. *Exercises: composability — the reason the pipeline must stay one-shot and side-effect-free except for the final send.*

## 3. Non-Goals

- Spawning agents on a folder (`atm spawn`) — different UX, folder is a workdir.
- Queueing work (`atm queue`) — same plumbing, different page; later.
- Replacing the OS-native Share sheet; we add an entry, we don't own the sheet.
- Binary transport inside the message bus. Messages carry references, not bytes.
- Designing Wyvern's chat/session integration. Phase 2b *consumes* it with a minimal contract; it does not define it.

## 4. Design

### 4.1 Pipeline (the whole product)

```
atm teams --json --members \
  | wyvern pick-member.html --stdin-json --out-json \
  | atm send --attach "$@" --from-json
```

Wyvern is used as a `dialog`/`zenity`-class picker: CLI-hosted webview, custom HTML/JS, no business logic. File paths never enter the UI; they ride `$@` directly into `atm send`.

### 4.2 Contracts

**Input to picker** (`atm teams --json --members`, the AQ2 projection; plain
`atm teams --json` remains the existing team-count output):
```json
{ "teams": [ { "id": "…", "name": "…",
    "members": [ { "id": "…", "name": "…", "host": "…", "cwd": "…",
                   "status": "active|idle|dead" } ] } ] }
```
`status` drives greying-out so a human can't send into a void.

`host` is an optional durable roster binding, written by the existing
`atm teams add-member`/`update-member --host` path and validated against the
enabled trusted-peer configuration. It is not inferred from heartbeat, DNS,
socket addresses, or the current process. A null host is displayed as
unroutable; `--from-json` rejects that recipient until an operator registers a
host or supplies an explicit canonical `agent@team.host` target.

**Output from picker** (stdout):
```json
{ "recipients": ["member-id", "…"], "note": "optional one-liner" }
```
Multi-select over recipients. `note` is a free-text "why" that travels with the attachment.

**Cancel:** Wyvern exits non-zero, emits no JSON, pipeline halts. No partial sends.

### 4.3 Envelope change

Add `attachments: []` to `MessageEnvelope`. **No new `MessageKind` verb** — the three-verb collapse stands.

```json
{ "sha256": "…", "size": 0, "name": "…", "kind": "file|dir",
  "origin_host": "…", "origin_path": "…", "local_path": "…" }
```
`local_path` is populated by the *receiving* daemon once bytes land.

### 4.4 Transport: pull, not push

- **Same host:** sender copies into the known agent-accessible temp area, `<known-temp>/atm/<msg-id>/`. Envelope references it.
- **Cross-host:** envelope carries `{sha256, size, origin_host, origin_path}`
  through the accepted transport stack — ADR-035 canonical write ingress
  under ADR-047 layered peer-wire security (`PeerWireMode`, mTLS default),
  with ADR-034 as the single-router HTTP shape reference. AQ1 chooses and records the authenticated peer byte-fetch endpoint;
  AQ3 implements it. The receiving daemon fetches into its own
  `<known-temp>/atm/<msg-id>/`, verifies hash and size, and only then makes the
  message readable. Sender code holds no fetch, SSH, or retry state. ADR-028
  and ADR-031 are historical/superseded references, not authority for this
  design.
- **Fan-out:** N recipients → N envelopes client-side; content-addressing makes repeated pulls cheap and deduplicable.

### 4.5 Lifecycle

Daemon-owned sweeper on `<known-temp>/atm/`. AQ1 closes the policy (TTL,
on-ack, or both) and names the configured root; AQ4 implements exactly that
decision. A shared well-known folder with no owner is a guaranteed leak.

### 4.6 Shell integration (thin glue only)

| Platform | Mechanism | Cost |
|---|---|---|
| macOS (first) | Shortcuts / Quick Action invoking the pipeline script | Low |
| macOS (later) | Share Extension — requires signed app bundle + extension target; depends on atm-daemon code-signing work | High |
| Windows (first) | `%APPDATA%\Microsoft\Windows\SendTo\*.lnk` | Trivial |
| Windows (later) | Win11 context menu (sparse MSIX) | High |
| Linux/Ubuntu (first) | Nautilus script (`~/.local/share/nautilus-scripts/`) + XDG `.desktop` "Open With" entry | Trivial |
| Linux (later) | KDE Dolphin service menus, other file managers | Low |

## 4a. Phase 2 — Agent-Assisted Drafting and Sessions

### 2a. One-shot prefill (fits the existing pipe)

```
atm teams --json --members \
  | atm draft --attach "$@" --model <fast> --merge \
  | wyvern pick-member.html --stdin-json --out-json \
  | atm send --attach "$@" --from-json
```

`atm draft` reads the attachment(s) with a fast model and adds `{summary, suggested_note}` to the picker input. The picker prefills the note field. The human edits or accepts. The model never runs inside Wyvern's JS.

Constraints:
- **Non-blocking.** Picker opens immediately; draft streams into the field. The ~1 s context-menu budget is not spent on inference.
- **Text-only sniff, byte cap.** Directories, binaries, and oversized files get a metadata-only summary (names, sizes, types).
- **Local model default** (Ollama/MLX on the Mac Studio), cloud fast model (Haiku) opt-in. Arbitrary files must not leave the machine by default.
- **Envelope `note_source: "human" | "drafted" | "edited"`** so recipients can weight it.

### 2b. Interactive drafting (first consumer of Wyvern chat integration)

Wyvern's planned chat window (fork a session or launch new) replaces the text field with a conversation. **Minimal contract this feature needs, and no more:**

| Op | Direction | Payload |
|---|---|---|
| `open_session` | page → host | `{ mode: "new" \| "fork", from_session?: id, attachments: [...] }` |
| `send_turn` | page → host | `{ text }` |
| `stream` | host → page | token/chunk events |
| `done` | page → host | `{ note }` — merged into output JSON |

Session-mode mapping:
- **Send to existing member → `new`.** The file is the only context; an ephemeral drafter session is correct.
- **Brief a freshly spawned agent → `fork`** from the session doing the briefing, so the kickoff inherits context. *This is the justification for fork existing at all.*

### 2c. Non-ATM session (US-3)

A second shell entry, "Open with agent", that skips `atm teams` and `atm send` entirely:

```
wyvern chat.html --attach "$@"
```

Same Wyvern capability, same attachment handling, no message bus. This is the simplest possible consumer and should probably ship *before* 2b as the integration smoke test.

### 2d. Chaining (US-4)

No new machinery. The guarantee that makes it work: **every stage is one-shot, reads stdin, writes stdout, and has no side effects except the final `atm send`.** Any stage that breaks this (a picker that sends on its own, a drafter that writes to the inbox) breaks chaining. Treat it as an invariant, enforced by `sc-lint` if it can be expressed.

## 5. Requirements

| ID | Requirement | Priority |
|---|---|---|
| R1 | One gesture from file manager to delivered message, macOS + Windows + Linux (Ubuntu/GNOME first) | Must |
| R2 | Multi-select recipients; multi-file via `$@` | Must |
| R3 | Cross-host delivery with hash verification | Must |
| R4 | Dead/idle members visibly disabled in picker | Must |
| R5 | Cancel never results in a send | Must |
| R6 | `atm teams --json --members` and `atm send --from-json` usable without Wyvern (TUI, Raycast, scripts) | Must |
| R7 | Sweeper reclaims inbox space per policy | Should |
| R8 | Attachment contents flagged as untrusted in agent conventions (CLAUDE.md) | Should |
| R9 | Phase 2: draft never blocks picker open | Must (P2) |
| R10 | Phase 2: local model default; cloud requires explicit flag | Must (P2) |
| R11 | Phase 2: `note_source` on envelope | Should (P2) |
| R12 | Phase 2: "Open with agent" entry works with zero ATM daemons running | Must (P2) |
| R13 | Pipeline stages are side-effect-free except final send (chaining invariant) | Must |

### 5a. Phase-1 command and envelope contract

The phase-1 shell contract is executable without Wyvern:

```text
atm teams --json --members -> PickerInput JSON
picker(PickerInput, "$@") -> PickerOutput JSON or non-zero/no output
atm send --attach PATH... --from-json < PickerOutput
```

`PickerInput` is the nested team/member object above. `PickerOutput` is
exactly `{"recipients":["member-id",...],"note":"optional"}`; unknown
keys, empty recipients, malformed JSON, or a cancelled picker are hard
failures and must not stage files or invoke the daemon. `atm send
--from-json` performs client-side fan-out through the existing canonical write
path, one immutable message per recipient. The AQ1 ADR owns message-id
allocation versus staging; AQ2 tests that order rather than inventing a
second rule.

All attachment bytes remain outside the message envelope. The envelope carries
references only; `local_path` is receiving-daemon-owned and is absent until
hash/size verification succeeds. This is the production boundary for both
same-host and cross-host paths.

## 6. Open Questions (block ADR, not prototype)

1. Directories: send as reference (`kind: dir`, recursive pull) or tar at origin?
2. Size limit, and what happens above it (refuse / warn / chunk)?
3. Sweeper policy: TTL vs on-ack vs both.
4. Team-level addressing in atm-core, or stay with client-side fan-out?
5. Wyvern cold-start latency — is it under the ~1 s context-menu tolerance? **Measure before committing Wyvern as the picker.**
6. Which registration fields supply member `cwd`? Host sourcing is no longer
   open: AQ1 decision (h) owns the explicit roster `host` binding and AQ2
   implements its projection/resolution without heartbeat changes.
7. (P2) Which local model is the drafter default, and what is "Luna"? Does it run on the Mac Studio via Ollama/MLX?
8. (P2) Does `fork` in the Wyvern chat integration fork the *session transcript* or the *agent process*? Send-To only needs transcript.
9. (P2) Byte cap for the drafter — and does a directory get a tree listing or nothing?

## 7. Milestones

1. **ADR-054 `attachments`** — envelope field, pull semantics, lifecycle decision.
2. **`atm teams --json --members` + `atm send --attach --from-json`** — testable with `echo '{"recipients":[…]}' |`.
3. **macOS Shortcuts prototype** using `osascript choose from list` — validates the workflow with zero UI work.
4. **`pick-member.html` in Wyvern** — replace step 3's picker; measure latency.
5. **Windows SendTo `.lnk`**.
6. Same-host → cross-host pull.
7. Sweeper.
8. **(P2)** `wyvern chat.html --attach` — "Open with agent", no ATM. Integration smoke test for chat window.
9. **(P2)** `atm draft` one-shot prefill, local model.
10. **(P2)** Interactive drafting via chat contract; `new` then `fork`.

## 8. Success

**Phase 1:** a file dropped on the shortcut appears in the right agent's inbox on another host, with its note, within a few seconds, and the human never opened a terminal. Nothing leaks in `<known-temp>/atm/` after a week.

**Phase 2:** US-1 through US-4 each complete in one gesture plus at most one edit. A chain of three hops works with no stage aware it's in a chain.
