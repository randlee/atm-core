# Sprint AQ2 — CLI Surface: Picker Projection, Fan-Out Send, Staging

Status: draft · Branch: `feature/aq-2-cli-surface` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Makes the pipeline real:
`atm teams --json --members | <picker> | atm send --attach "$@" --from-json`.
Staging is a local copy or a transfer-script invocation (AQ1 decision (c));
the daemon carries only ordinary messages.

## Deliverables

1. **Picker projection for `atm teams --json --members`** (build, not audit —
   verified baseline: today's output is `{name, member_count}` per team with
   no member entries). Add the `--members` projection emitting the PRD §4.2
   nested projection `{id, name, host, cwd, status}` per member, sourced
   from the members surface (`MemberSummary`: `agent_id`, `home_dir`,
   `live_cwd`, pane) and runtime state. Normative status mapping from
   `RuntimeMemberState`: `Active → active`, `Idle → idle`,
   `Offline | Unknown | IdentityConflict → dead`. `host` is sourced only
   from AQ1 decision (e)'s durable roster binding; AQ2 adds the `--host`
   admin metadata plumbing to write/read that field, but does not change
   heartbeat or daemon runtime code. Missing host is emitted as `null` and
   the picker marks the member unroutable.
2. **`atm send --attach <path>...`** (repeatable): same-host recipients →
   copy into `send_to_staging_dir()` (AQ1) under the local `$ATM_TEMP`;
   remote recipients → resolve `~/.atm/transfer/<host>` and invoke it per
   the AQ1 decision-(c) contract — argv-array exec, bounded deadline with
   child kill on expiry, capped stdout/stderr, single-line absolute-path
   stdout validation — grouping recipients by host (one transfer per
   destination host per invocation), transfers executed **sequentially per
   host** (bounded, no unbounded concurrent child processes from one
   fan-out). Missing/unreadable source path → hard error before any staging
   or send.
3. **Transfer failure semantics (R3/R5/R13)**: a missing transfer script
   yields the canonical AQ1 error verbatim (`File transfer to <host> not
   enabled. Read docs/cross-host-file-transfer.md …`); a failing script's
   stderr is propagated to the user verbatim. Either case aborts the whole
   invocation with exit ≠ 0 and **zero messages sent** — the user always
   sees why.
4. **`atm send --from-json`**: reads the `PickerOutput` schema from stdin;
   fan-out is one ordinary message per recipient (new — send is
   single-recipient today, positional `to` required); the note plus the
   AQ1 decision-(d) path template become the message text. Requires making
   positional `to` and `message` optional at the clap level; mutually
   exclusive with positional `to`, `--stdin`, `--file`, and `--template`;
   conflicts rejected by clap, not runtime checks. Delivery uses the
   existing canonical daemon write path per recipient — no direct storage
   writes, no daemon changes.
5. **Cancel semantics (R5/R13)**: empty stdin or malformed JSON → nonzero
   exit, zero sends, zero files staged, zero transfer-script invocations.
   All recipients and all attachment paths validate before any staging or
   transfer begins.

## Normative CLI contracts

The new picker projection is exposed as `atm teams --json --members`; the
existing `atm teams --json` team-count shape remains backwards-compatible.
Each member object is exactly:

```json
{"id":"agent-id","name":"display-name","host":"host-or-null",
 "cwd":"absolute-or-null","status":"active|idle|dead"}
```

The source record is explicit: `RosterEntry.metadata_json["host"]` stores an
optional validated `HostName`, `MemberSummary` projects it as
`host: Option<HostName>`, and `teams add-member/update-member --host` is the
only Phase-1 writer. No heartbeat or live socket observation may populate it.

`atm send --from-json` accepts exactly one JSON object on stdin — the
`PickerOutput` schema (the name AQ5 and the PRD §4.2 picker contract bind to):

```json
{"recipients":["member-id", "member-id-2"], "note":"optional text"}
```

The parser rejects unknown keys, empty recipients, duplicate recipients,
malformed JSON, and trailing non-whitespace.

Recipient resolution is explicit and testable:

```rust
fn resolve_picker_recipient(
    member_id: &str,
    roster: &dyn RosterStore,
) -> Result<AgentAddress, AddressResolutionError>;
```

The resolver maps a null-host member to the local canonical team address and
a registered host to `agent@team.host`. It never derives host from heartbeat,
DNS, socket family, or the picker UI. (No `TrustedPeer` check here — message
delivery uses the existing peer path unchanged, and file transfer is governed
by the transfer script's own auth.)

`AddressResolutionError` inventory (variants normative; codes/messages
finalized in-sprint):

| Variant | Cause | Recovery |
|---|---|---|
| `UnknownMember` | id not in roster | pick a rostered member or `atm members` to list |
| `HostUnregistered` | member has `host: null` but a remote send is required | `atm teams update-member --host <host>` or use explicit `agent@team.host` |

Message-text template (AQ1 decision (d)), rendered per recipient:

```text
<note>

Attached files (on this host):
  <landed-dir>/<basename>
  ...
```

## Acceptance criteria

1. Truth-table tests for `--from-json`: valid multi-recipient → N ordinary
   messages, each naming the landed paths; empty/malformed/cancel → exit ≠ 0,
   zero staging dirs created, zero transfer invocations.
2. Same-host end-to-end test: file lands under
   `$ATM_TEMP/send-to/<transfer-id>/`, recipient's message text names the
   landed path, file content matches the source.
3. Missing transfer script for a remote recipient → the canonical AQ1 error
   verbatim on stderr, exit ≠ 0, zero sends. Failing transfer script (stub
   exiting 1 with stderr) → its stderr propagated verbatim, exit ≠ 0, zero
   sends.
4. Remote-recipient happy path with a stub transfer script (records argv,
   emits a destination dir): one invocation per destination host; message
   text carries the stub's reported dir.
5. `atm teams --json --members` output validates against the picker input
   schema in the PRD (§4.2) via a fixture test.
6. Legacy single-recipient `atm send <to> <message>` still requires both
   positional arguments and preserves its existing malformed/missing-input
   exit codes/messages after the parser makes them optional for `--from-json`.
7. `just test` all three CI lanes (ubuntu, macOS, Windows); no clippy
   warnings in touched crates.

## Paths to delete

None. AQ2 extends the CLI and adds staging/transfer-invocation code; it must
not delete the existing single-recipient `atm send` mode, alter legacy
team-count output, or touch daemon runtime code.

## Required validation

- `just test` workspace, ubuntu + macOS + Windows CI lanes.
- One recorded same-host demo transcript (command + resulting message text)
  committed as evidence on the sprint branch.
- Focused command tests for `atm teams --json --members` and
  `atm send --from-json` named in the PR, run independently of Wyvern and of
  any real SSH configuration (stub scripts only).

## Non-closure / out of scope

- Real transfer scripts and setup doc examples (AQ3). Sweeper (AQ4). UI (AQ5).

## Dependencies

- must_follow: AQ1 (contract) — merge-forward before every dev/fix round.
- Dispatch precondition: `integrate/phase-aq` created from `develop`.
- parallel_safe: none before AQ2 merges; AQ3/AQ4/AQ5 fan out after.
