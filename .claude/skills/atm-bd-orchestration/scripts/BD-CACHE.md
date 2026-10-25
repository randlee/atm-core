# bd-cache: per-dolt-database bead cache and write journal

Design tracker: randlee/atm-bd-orchestration#557. Reference implementation:
`bd_cache.py` (Python) with `tests/test_bd_cache.py` and
`tests/test_bd_cache_cron.py`. A Rust crate (separate `bd-cache` repository,
its own team) replaces the Python module behind the same CLI, JSON and SQLite
schema. The Python tests are its acceptance suite. This document is the
contract for that implementation. Where the Python code and this document
disagree, that is a defect in one of them: report it on #557.

## Goals

1. **Hooks never wait on dolt.** ATM hooks (6-8 planned; the sanity
   `task.close` hook is the first) run under a 2.5 s budget shared with their
   ATM actions. `bd` costs 130 ms to several seconds per call under host load,
   and dolt stalls (17-20 s connection handshakes observed 2026-10-23). Hook
   reads come from local SQLite, and hook writes go to a local journal, so a
   slow or failing backend never fails or stalls a close.
2. **No silent failure.** Every write either lands in `bd` (verified by a
   read-back), or is escalated to the phase lead with the exact fix. Nothing
   is dropped, retried forever or guessed.
3. **Exactly the effect, once.** A retried hook never journals a write twice
   (idempotency keys). A write that already landed is never repeated
   (`achieved`). A write whose precondition no longer holds is never applied.
4. **Backend-neutral.** All backend access goes through the `bd` CLI, so the
   cache works with every backend `bd` supports. It never reads dolt tables.
5. **Strict contract with `bd`.** Rows are validated against models generated
   from `bd schema`. A `bd` upgrade that changes the row shape fails loudly
   (`BD_CACHE.BD_CONTRACT`), never as silent misreads.

## How it works

```
 hook / console ──read──▶ SQLite cache (row + pending journal overlay)
       │                        ▲            ▲
       └──write──▶ journal      │ sync       │ read-back
                      │         │ (bd list)  │ (bd list --id)
                      ▼         │            │
                 drain ──bd close/update/dep add──▶ bd ──▶ dolt
                   ▲
   started detached by the hook on return; also run by the bd-cache cron every 30 s
```

- **Read:** `get` returns the cached row with that bead's pending journal
  entries replayed on top. On a cache miss it makes one `bd list --id` call
  and stores the row.
- **Write:** `write` validates the operation, checks its precondition against
  the current view, and inserts a `pending` journal entry. It never calls `bd`
  except to fill a cache miss.
- **Drain:** one writer at a time (`flock`). It re-reads every pending bead
  in one batched `bd list --id` call, skips writes already achieved,
  escalates writes whose precondition fails, runs the `bd` command, and stores
  the read-back row.
- **Sync:** incremental `bd list -l phase-<x> --updated-after` keeps cached
  rows current for writes made outside the cache (agents' own `bd` calls).
- **Cron:** the `bd-cache` ATM cron drains, syncs the configured phases, and
  sends every escalation and new sync failure to the phase lead, once.

Agents' own `bd show` and `bd ready` calls read the backend directly. They do
not see a journaled write until the drain applies it: normally under a second
(the detached drain), at most one cron tick (30 s) when the detached drain
could not start or the backend is slow. During that window the backend shows
the state before the close. For the sanity hook, this lag delays work but
never releases it early. Before the drain, a PASS looks like a sanity bead
that is still open, so its QA is not yet ready. A FAIL looks like a closed
dev bead with an open sanity bead, so nothing new is ready. A future hook
that journals a write which releases work must state its own lag behaviour
here.

## Performance

Measured on rand-m5 on a live team database, 2026-10-23, `bd` 1.3.1, proxied dolt
sql-server. Load average is given for each measurement, because `bd` latency
scales with it.

| Operation | Measured | Target (Rust) |
|---|---|---|
| In-process cached `get` | 0.017 ms median, 0.14 ms max (load 7-10) | < 1 ms |
| New-process CLI `get` (`python3 bd_cache.py get`) | 129 ms median (load 7-10); 80 ms (load 3.5) | < 50 ms |
| Hook process start + import (`sanity_task_close_hook`, imports pydantic) | 70-80 ms (load 3.5) | Rust binary: no interpreter or pydantic cost |
| `bd list --id <one bead>` | 160-180 ms (load 3.5); `bd show` 134 ms median, 254 ms max (load 7-10) | n/a (backend) |
| First full sync, one 810-bead phase | 0.42 s (`bd version` + `bd schema` + one `bd list`) | — |
| Incremental sync, the same phase | 0.20 s | < 1 s for a 1000-bead phase |
| Drain | 3 `bd` calls per pass (`bd version`, `bd schema`, batched pre-read), then 2 per entry (write, read-back); 4 entries on 3 beads in 0.85 s | — |

**Hook budget (sanity close, 2.5 s).** Process start plus imports take about
80-130 ms. The routing read is a cache hit (< 1 ms). The hook makes one
batched `bd list --id` of the sanity and dev beads (about 0.2 s, capped at the
remaining budget minus 0.5 s). If that refresh fails, the hook decides on the
cached rows and names the error as `read_fallback`. Journal inserts are local
SQLite (milliseconds). It spawns the drain detached and returns. A typical
close spends about 0.3-0.5 s of its 2.5 s budget.

**Drain bounds.** Each `bd` call has a 30 s timeout. The cron tick has a 25 s
budget and gives the drain a deadline 5 s before it ends. That deadline is a
start gate, not a hard bound: it is checked before each entry starts, so an entry that has started runs to
completion. In the worst case (every call hits its timeout), that is one
write plus one read-back, 60 s, which exceeds the daemon's 30 s cron timeout.
ATM then kills the tick. This is safe: `flock` is released when the process
exits, and the entry stays `pending`. If the write landed, the next pass finds
it `achieved` and records it without a second write. If it did not, the entry
retries until `MAX_ATTEMPTS` (5), then escalates. A killed write is an attempt; an
unreachable backend is not (see Failure handling, outage). The Rust implementation
should add a hard deadline: cap each `bd` call at the time left before it, so
a tick never exceeds its budget.

**Cache size.** The measured file holds 1887 rows across phases bd, bj and bk
(7.6 MB with WAL). Closed phases stay until `prune` runs (see Open items).

## Failure handling (every path ends in applied, refused or escalated)

| Failure | Where | Outcome | Who fixes |
|---|---|---|---|
| Invalid op, args or precondition at write time | `write` | typed error to the caller; nothing journaled | caller |
| Precondition unmet at write time | `write` | `BD_CACHE.PRECONDITION` with `unmet {key: {expected, actual}}` | caller re-reads and decides |
| Same idempotency key, different write | `write` | `BD_CACHE.IDEM_CONFLICT` | caller |
| `bd` slow or down while a hook runs | hook | decides on cached rows, `read_fallback` named in any escalation; writes journaled | none; the drain applies them later |
| Drain cannot start after a hook | hook | stderr record `drain_spawn_error`; the cron drains within one tick | none |
| Bead absent at drain | drain | escalated | phase lead |
| Precondition no longer holds at drain | drain | escalated, no write | phase lead |
| `bd` guard refused (exit 13) | drain | escalated | phase lead |
| Backend unreachable (`bd` cannot start, times out, or a list exits nonzero) during the backend check or the batched pre-read | drain, sync, cron | an outage: recorded once with its start time, every entry stays pending with no attempt counted, per-phase sync alerts suppressed; sent once per outage to every active phase lead; cleared by the next `bd list` that answers | whoever restores bd |
| `bd` write fails or times out | drain | retried; later entries for that bead held; escalated at 5 attempts | phase lead after 5 |
| Read-back failure | drain | applied, row marked `stale`; the bead's later entries held until a sync re-reads it | none |
| Sync failure from a reachable `bd` (e.g. `BD_CONTRACT`) | cron | `BD_CACHE.SYNC_FAILED` recorded; sent once per cause to the phase lead | phase lead |
| `bd` schema changed | backend check | cache rows rebuilt, journal kept; `BD_CONTRACT` if rows no longer validate | maintainer regenerates `bd_models.py` |
| Escalation message not delivered | cron | reply `refused` with the list; retried every tick | ATM operator |

Every escalation to the phase lead names the entry (`seq, bead, op, actor`),
the error, and the recovery: check the bead in `bd`, set the intended state,
then run `bd_cache.py resolve <seq> --note <what you did> --actor <you>`.
`status` and the cron's `doctor` list every escalated entry until it is
resolved.

## Scope

In: one SQLite file per dolt database, every active phase in it; read API;
journal write API; drain; incremental sync; prune; status; typed errors.

Writers: `sanity-close.py` journals and leaves the drain to a detached
`bd_cache.py drain` or the cron. `atm-task-close.py` journals each bead update
and close, keyed by vars, assignment and op. It drains at once, waiting up to
5 × 2 s on a busy drain. An entry still pending is `BEADS.WRITE_PENDING`
(rerun with the same vars; the key keeps it applied once), and an escalated
entry is `BEADS.WRITE_ESCALATED`. `close-vars.py` reads the cache and re-reads
from bd only when a field the vars need is missing there.

Out (v1): creating beads (pours stay `bd import`/sc-compose), comments,
dependency removal, float/null/structured metadata values. Agents' own `bd show`/`bd
ready` keep reading the backend; they lag journaled writes until the drain
applies them.

## Storage

File: `<repo>/.atm-bd/<dolt_database>.bd-cache.sqlite3` (gitignored), e.g.
`my-team.bd-cache.sqlite3`. The name and the project id come from
`.beads/metadata.json` (`dolt_database`, `project_id`; `$BEADS_DIR` when
set), so locating the file starts no `bd` process. Missing or invalid
metadata is `BD_CACHE.NO_DATABASE`. WAL mode, busy timeout 5 s,
`PRAGMA user_version` = cache schema version (3). A file with another
nonzero version is refused (`BD_CACHE.SCHEMA_VERSION`); version 0 is
initialized.

```sql
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE beads (
  id TEXT PRIMARY KEY, phase TEXT, status TEXT, updated_at TEXT,
  row_json TEXT NOT NULL,          -- normalized bead, see "Bead shape"
  synced_at TEXT NOT NULL,
  stale INTEGER NOT NULL DEFAULT 0 -- 1 = read-back failed; next sync re-reads it
);
CREATE INDEX beads_phase ON beads(phase, status);
CREATE TABLE edges (
  issue_id TEXT NOT NULL, depends_on_id TEXT NOT NULL, type TEXT NOT NULL,
  PRIMARY KEY (issue_id, depends_on_id, type));
CREATE INDEX edges_target ON edges(depends_on_id, type);
CREATE TABLE journal (
  seq INTEGER PRIMARY KEY AUTOINCREMENT, idem_key TEXT NOT NULL UNIQUE, phase TEXT,
  bead TEXT NOT NULL, op TEXT NOT NULL, args_json TEXT NOT NULL, precondition_json TEXT NOT NULL,
  actor TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('pending','applied','escalated','resolved')),
  attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  announced_at TEXT);               -- escalation sent to the phase lead
CREATE INDEX journal_pending ON journal(state, bead, seq);
CREATE TABLE sync_state (
  phase TEXT PRIMARY KEY, last_started_at TEXT, last_ok_at TEXT, last_error TEXT);
```

`phase` is the suffix of the bead's first `phase-<x>` label. Timestamps are
RFC 3339 UTC, second precision.

`meta` keys:

| key | written | use |
|---|---|---|
| `dolt_database`, `project_id` | first open | a later open whose metadata.json `project_id` differs is `BD_CACHE.WRONG_DATABASE` (the file was built from another database) |
| `bd_schema_sha256`, `bd_version`, `bd_schema_version` | backend check | rebuild rule below |
| `sync_alert:<phase>` | cron, after a sync failure is sent | the cause last announced; the same cause is not re-sent, success deletes the key |

Backend check (start of every `sync` and of every non-empty `drain`):
`bd version` and `bd schema`; sha256 of the `bd schema` stdout. The first
check records it. A recorded sha that differs (a `bd` upgrade while a team
sat idle on a warm cache) deletes every bead, edge and sync cursor in one
transaction, keeps the journal, and records the new sha; the next sync
refetches whole phases and pending entries replay on the new rows. The check
reports `rebuilt` and `model_current` (sha equals the one `bd_models.py` was
generated from). `bd context` is not used: bd 1.3.1 refuses it for
workspaces under temp directories ("unsafe location") that every other `bd`
command accepts; identity comes from metadata.json instead.

## Bead shape (the contract with `bd`)

Every backend read is `bd list --id a,b,... --status
open,in_progress,blocked,deferred,closed,pinned,hooked -n 0 --json` (or the
phase-label form in sync); `bd show` is never called, so there is one row
shape. Its `dependencies` are already outgoing edges
`{issue_id, depends_on_id, type}`.

Each row is validated by `CachedIssue`: `bd_models.Issue`, generated from
`bd schema` by datamodel-code-generator (pydantic v2, `extra=forbid`; the
regenerate command is in the file header, which pins `BD_VERSION`,
`BD_SCHEMA_VERSION` and `BD_SCHEMA_SHA256`), plus the four keys `--json`
emits that `bd schema` does not declare: `comment_count`,
`dependency_count`, `dependent_count`, `parent` (`bd show` also adds
`revision`; not read). An unknown key, missing required key or wrong type is
`BD_CACHE.BD_CONTRACT` naming the bead and field; a sync with one bad row
stores nothing. Stored rows drop `dependency_count` and `dependent_count`
(derived) and default `labels`/`dependencies` to `[]`, `metadata` to `{}`.

Contract fixtures: `tests/fixtures/bd-1.3.1/bd-schema.json` (sha must equal
`BD_SCHEMA_SHA256`) and `list-rows.json` (recorded rows that must validate).
A `bd` upgrade that changes the schema fails the sha test until
`bd_models.py` is regenerated. Requires `pydantic` 2.

## Journal operations

| op | args | overlay | `bd` command |
|---|---|---|---|
| `close` | `{reason?}` | status=closed, close_reason=reason | `bd close <id> [--reason R] --actor A` |
| `update` | nonempty subset of `status, assignee, add_labels[], remove_labels[], set_metadata{str:int\|bool\|str}, unset_metadata[], append_notes` | each field applied in that order | `bd update <id> --status … --assignee … --add-label … --remove-label … --set-metadata k=v … --unset-metadata k … --append-notes T [--if-status S] [--if-assignee U] --actor A` |
| `dep_add` | `{depends_on, type}`; type one of bd's dependency types (`bd_models`), else `BAD_ARGS` | add edge if absent | `bd dep add <id> <depends_on> --type T --actor A` |

Precondition: optional `{status?, assignee?}`; `""` means unassigned/empty.

Metadata values must survive `bd`'s coercion unchanged (bd 1.3.1, probed
live): `--set-metadata k=v` stores `5`, `5.0`, `1e3` and ` 7` as integers,
`true`/`false` as booleans, `null` as null, and keeps `[1]`, `{"x":1}`,
`"5"` and `True` as strings. So a value is accepted only as an int, a bool,
or a string without surrounding whitespace that does not parse as a JSON
number, boolean or null; anything else is `BD_CACHE.BAD_ARGS`. Bools are
passed as `true`/`false`. A string like `"5"` cannot be stored through
`--set-metadata` at all (an all-digit short SHA would become a number).

`achieved(row, op, args)` (the effect is already in the backend):
close → status closed; dep_add → edge present; update → every requested
field already holds (labels added present, removed absent, metadata keys
equal / absent, notes contain the appended text).

`idem_key` is required and derived by the caller from the whole operation
(e.g. digest of team, task, assignment id, verdict). Re-writing the same key
with the same `(bead, op, args)` returns the existing entry (`duplicate:
true`); with a different write it is `BD_CACHE.IDEM_CONFLICT`.

## Algorithms

`get(bead)`: cached row, or on miss one `bd list --id <bead>` (caller-supplied
timeout) and store it; absent in `bd` too → `BD_CACHE.NOT_FOUND`; then replay that bead's `pending` entries in `seq` order with the
overlay. Escalated entries are not replayed.

`write(...)`: validate op/args/precondition; dedupe on `idem_key`; read the
current view (`get`); unmet precondition → `BD_CACHE.PRECONDITION` with
`unmet {key: {expected, actual}}`, nothing journaled; else insert `pending`.

`drain(deadline=None)`: exclusive non-blocking lock on
`.atm-bd/<dolt_database>.bd-cache.drain.lock` (held → `{busy: true}`). An
entry not started by the deadline (a monotonic time) is reported in
`deferred`, stays pending and keeps its `attempts`. With pending entries: backend check, then one
batched `bd list --id` of every bead with a pending entry. An unreachable
backend in either (`UNREACHABLE`: `BD_UNAVAILABLE`, `BD_VERSION_FAILED`,
`BD_SCHEMA_FAILED`, `BD_LIST_FAILED`) records the outage and returns
`{ok: false, outage, deferred: <every seq>}` with no attempt counted; any other
pre-read failure → every entry retries. For each pending entry by `seq`, skipping beads whose earlier
entry failed in this pass:

1. bead absent from the batch → escalate;
2. `achieved` → store fresh row, mark applied (no second write);
3. precondition unmet on fresh → store fresh row, escalate;
4. run the command; exit 13 (`bd` guard refused) → escalate;
   other failure/timeout → `attempts += 1`, stays pending, bead held for this
   pass; `attempts` reaching `MAX_ATTEMPTS` (5) → escalate;
5. read back with `bd list --id <bead>`; in one transaction store the row
   (forced) and mark applied; the row becomes the bead's fresh state for its
   next entry. Read-back failure → mark applied with the error, set
   `stale = 1`, and hold the bead's later entries until a sync re-reads it.

Each `bd` call has a 30 s timeout. The report lists `applied`, `retrying`
`escalated` (`seq, bead, op, actor, phase, reason`) and `deferred`, plus
`outage` when the backend was unreachable.

Outage state lives in `meta`: `outage_since` (kept from the first failure),
`outage_error` (the latest) and `outage_announced_at`. A drain pre-read or
sync list that answers clears all three; `bd version` passing does not,
because on a loaded host it answers while lists still time out. A sync whose
backend check or list is unreachable records the outage and raises
`SYNC_FAILED` with `cause` set to the underlying code.

`sync(phase)`: backend check, then `bd list -l phase-<x> -n 0 --status
<all seven> --json`, every row validated before any is stored, with `--updated-after
(last_ok_at - 2 min)` once a sync has succeeded. Upsert each row unless the
stored `updated_at` is newer (drain read-backs are forced). Then re-read
`stale` rows with one `bd list --id`. `last_ok_at` is the sync's start time. Failure →
`BD_CACHE.SYNC_FAILED`, recorded in `sync_state.last_error`.

`resolve(seq, note, actor)`: the lead set the bead by hand after an
escalation; the entry becomes `resolved` (never replayed), the note is
appended to `last_error`. Anything but an escalated entry is
`BD_CACHE.NOT_ESCALATED`.

`prune(phase)`: refused while the phase has pending or escalated entries;
else deletes its beads, edges, journal and sync state. Run after the phase
root closes.

## Cron module (`bd_cache_cron.py`, ADR-069)

One schedule per repository, started from the beads root with
`custom = {"team": <ATM team of the phase leads>}`; `TIMEOUT_MS` 2500,
`CRON_TIMEOUT_MS` 30000. Phases are `.atm-bd/phase-*.toml` (`root`, `lead`);
a phase is active while its root bead is not closed (uncached roots are read
in one `bd list --id`).

`cron.tick`, within a 25 s budget: drain with the deadline 5 s before the
budget ends; sync each active phase, deferring any not started 2 s before
it (an uncached root that cannot be read during an outage counts as
inactive); a sync failure whose code or `cause` is unreachable is not
announced per phase; any other sync failure whose cause differs from
`sync_alert:<phase>` is sent to that phase's lead, success clears it; an
outage not yet announced is sent once to every active phase's lead (every
configured lead when none is known active), then marked; every escalated entry with no
`announced_at` is sent to its phase's lead with the `resolve` command, then
marked. Messages go through `atm send <lead>@<team> --team tool` as `atm-bd@tool`
(the installer's member; `cron@tool` is atm's daemon-owned scheduler principal and refuses ordinary sends).
Any message not delivered (or an entry whose phase has no lead) makes the
reply `refused` with the list, and is retried next tick; otherwise
`pass-through`. Missing `custom.team` is refused.

`cron.stop`: stop when no phase is active and no entry is pending or
escalated-unannounced. `doctor`: `status`, with escalated entries as findings.

## CLI (JSON on stdout, one object)

```
bd-cache [--root DIR] [--db PATH] get <bead>
bd-cache list --phase X [--status S] [--label L]
bd-cache write <bead> <close|update|dep_add> --args JSON [--precondition JSON] --idem-key K --actor A
bd-cache drain
bd-cache sync --phase X [--phase Y ...]
bd-cache prune --phase X
bd-cache resolve <seq> --note TEXT --actor A
bd-cache status
```

Exit 0 ok; 2 usage; 3 typed error `{ok:false, code, detail, fix, ...}`.

| code | meaning | fix carried |
|---|---|---|
| `BD_CACHE.BD_UNAVAILABLE` | `bd` timed out or could not start | an outage before a write (no attempt counted); a timed-out write retries |
| `BD_CACHE.NO_DATABASE` | no readable `dolt_database`/`project_id` in metadata.json | run from the beads root or set `BEADS_DIR` |
| `BD_CACHE.WRONG_DATABASE` | file's recorded project id differs | move the file aside |
| `BD_CACHE.NOT_FOUND` | bead in neither cache nor `bd` | check the bead id |
| `BD_CACHE.BD_VERSION_FAILED` / `BD_SCHEMA_FAILED` | backend check failed | install `bd` / transient |
| `BD_CACHE.BD_CONTRACT` | row fails `CachedIssue`, or invalid JSON | regenerate `bd_models.py` from `bd schema`, update `CachedIssue` |
| `BD_CACHE.BD_WRITE_FAILED` | write nonzero (not 13) | retried by next drain |
| `BD_CACHE.BD_LIST_FAILED` / `SYNC_FAILED` | read or sync failed | retried by next run |
| `BD_CACHE.PRECONDITION` | write-time precondition unmet | re-read and decide again |
| `BD_CACHE.IDEM_CONFLICT` | key reused for another write | derive key from the whole op |
| `BD_CACHE.BAD_OP` / `BAD_ARGS` / `BAD_PRECONDITION` | invalid request | as named |
| `BD_CACHE.SCHEMA_VERSION` | file newer than code | upgrade, or move file aside to rebuild |
| `BD_CACHE.PRUNE_BLOCKED` | unapplied writes | drain / resolve escalations first |
| `BD_CACHE.NOT_ESCALATED` | `resolve` on an entry that is not escalated | resolve only entries listed in `status` |
| `BD_CACHE.NO_TOML` | cron module on Python < 3.11 | run it with Python 3.11+ |

`status` returns the `meta` table, journal counts per `phase:state`,
per-phase sync state and bead counts, and every escalated entry; the console
shows these.

## Rust shape

One crate, `bd-cache`, in its own repository: library + binary, distributed
like sc-compose (prebuilt release binaries, one pinned version per consuming
repo). Hooks and the cron call the binary (or link the library) in place of
`bd_cache.py`; the CLI, JSON replies, exit codes, error codes and the SQLite
file (schema version 3) are unchanged, so a Python-written file is read by
the Rust binary and the reverse.

- `Bead`: serde types generated from the same `bd schema` (deny unknown
  fields) plus the four `--json` extras; `Edge`.
- `enum Op { Close { reason: Option<String> }, Update(UpdateArgs), DepAdd { depends_on, kind } }`
  with `fn overlay(&self, &Bead) -> Bead`, `fn achieved(&self, &Bead) -> bool`,
  `fn command(&self, id, &Precondition, actor) -> Vec<String>` (all pure).
- `Precondition { status: Option<String>, assignee: Option<String> }`.
- `trait BdRunner { fn run(&self, argv: &[String], timeout: Duration) -> Result<Output, RunError>; }`:
  the only backend port. `ProcessBd` (real) and a deterministic fake for tests.
- `enum CacheError` with exactly the codes above; `fix()` text per variant.
- `bd version`/`bd schema` backend check and the meta rebuild rule as above.
- `rusqlite` with bundled SQLite; one connection per process.

## Tests

1. Unit (no `bd`, no dolt; fake `BdRunner` emitting validated list rows):
   port every case in `tests/test_bd_cache.py` (44 tests) and
   `tests/test_bd_cache_cron.py` (13 tests, real cache with the fake runner and an
   injected sender): file name from
   metadata.json and `BEADS_DIR`, NO_DATABASE, WRONG_DATABASE, schema change
   rebuilds rows and keeps the journal, first check keeps rows, model_current,
   BD_CONTRACT on a bad row with no partial store, one batched pre-read,
   unknown dep type refused, not-found escalation, miss-once, overlay
   visible but row untouched, ordered drain + read-back, transient failure
   holds only that bead, retry cap escalates, write-time precondition,
   backend change escalates without writing, exit-13 escalates, landed write
   not repeated, read-back failure → stale → sync refresh, idempotency key,
   incremental sync and newer-row guard, typed sync failure, edge index,
   single drain writer, prune guard, schema refusal, bad ops, drain
   deadline defers without attempts, announce/resolve bookkeeping, resolve
   keeps the note, prune after resolve, sync alert round trip, an
   unreachable backend (pre-read timeout, pre-read exit, backend check) counts
   no attempt across more than `MAX_ATTEMPTS` drains and recovers, outage
   keeps its start, a reachable refusal still counts, sync records and clears
   the outage; cron: only
   open-root phases sync, escalation sent once to the phase lead,
   undelivered escalation refused and retried, sync failure announced once
   per cause and cleared, missing team refused, stop rule, tick budget
   defers the drain, outage announced once (not per phase) and again for a
   new outage, uncached roots during an outage tell every configured lead,
   a passing `bd version` does not clear the outage, undelivered outage
   announcement retried. Each test must
   fail against a planted bug: the prototype was checked with 19 (among them: no overlay,
   no write/drain precondition, no newer-row guard, no per-bead hold, no
   achieved check, no retry cap, no stale flag, no drain lock, no metadata
   coercion guard, bools passed as `True`, skip validation, skip rebuild,
   rebuild deletes journal, skip project-id check, per-entry pre-read,
   rebuild on first check, no dep-type check, no not-found escalation); every
   one failed at least one test. The escalation and cron code was checked
   with 13 more (among them: prune ignores escalated, unannounced filter dropped,
   open-work ignores announcement, resolve accepts any state, deferred not
   reported, no mark_announced, no set/clear sync alert, refusal dropped,
   team check dropped, closed roots active, stop ignores open work, drain
   without deadline); every one failed at least one test. The outage code
   was checked with 7 more (outage counts attempts, per-phase alert on outage,
   outage start overwritten, announcement not marked, uncached root raises,
   sync never clears, sync check records nothing); every one failed at least
   one test, and a no-op control passed.
2. Contract: the bd-1.3.1 fixtures above; the fake runner's command plan is
   asserted.
3. Live integration (testbed only, never in CI or unit suites): sync, get and
   drain against a dolt sql-server, embedded dolt and a DoltHub remote; a
   25-bead random sample must equal fresh validated `bd list --id` rows.

Targets: in-process get < 1 ms; CLI get < 50 ms; incremental sync < 1 s for
a 1000-bead phase.

## Live proofs (2026-10-23)

Validating prototype (schema-validated, `list --id` reads), isolated
proxied-server database `ct` in a scratch directory, real `bd` 1.3.1: file
`.atm-bd/ct.bd-cache.sqlite3`, meta recorded (database, project id, pinned
schema sha, 1.3.1, schema 1), no `bd show`/`bd context` calls; overlay
visible before drain with the backend untouched; drain applied 4 entries on
3 beads in 0.85 s with one batched pre-read; fields, edge, close reason and
typed metadata (`{commit: "abc123", n: 3, ok: true}`) landed and cache rows
equalled `bd list --id`; second drain no-op; landed write not repeated;
concurrent assignee change escalated without a write; stale `--if-status`
exit 13, no write. Simulated schema change (recorded sha altered): sync
rebuilt and relisted, a pending close survived, overlaid as closed while
dolt said open, then drained and landed.

Read-only on the live team database (one phase, 810 beads): sync and a 25-bead random sample
equal to fresh validated `bd list --id` rows (0 differences); only `bd
version`, `bd schema` and `bd list` ran; no writes or drain.

First prototype (show-based), same scratch setup: run against an isolated proxied-server beads database in a scratch
directory (not the shared team database), real `bd` 1.3.1: four journaled
ops (update status/assignee with preconditions; labels + metadata + notes;
dep_add; close with reason) stayed out of dolt until drain and were visible
in reads; drain applied all four (4.6 s, about 1.2 s per entry = 3 `bd`
calls); every cache row equalled `bd show`; second drain no-op; a write that
had already landed was not repeated; a concurrent assignee change escalated
with no write; `bd update --if-status` with a stale status exits 13 and
writes nothing; typed metadata overlay equalled the stored values.

That prototype cost three `bd` calls per entry; the batched pre-read makes
it 2 + 2 per entry (backend check, one pre-read, then write and read-back),
and the fresh check before every write is kept.

## Open items

- Untested backends: embedded dolt (this `bd` build has no CGO) and DoltHub
  remotes.
- No escalation has been delivered through a live `atm send` yet. The tests
  inject the sender, and no live entry has escalated.
- The Python CLI `get` (80-129 ms) misses the 50 ms target. Python startup and
  the pydantic import dominate. The Rust binary is expected to meet it.
- A drain entry can outlive the cron tick (see Performance, Drain bounds).
  This is safe but wastes the tick. The Rust implementation caps each call
  at the time left.
- Blocked state is not cached. `bd list --json` rows carry no `is_blocked`, and
  bd settles blocked state without bumping `updated_at`, so a sync would miss
  it. Readiness always comes from live `bd ready`. A cache that serves ready
  must re-read the dependers of changed blockers, including blockers outside
  the synced phases (beads-cache ADR-BDC-0002).
- Closed phases are not pruned automatically. Run `prune --phase <x>` after
  the root closes. Automatic prune from the cron once a root is closed and
  its journal is clear is a candidate.
- Two journals: the sanity hook keeps close plans and error escalations in
  `.atm-bd/sanity-close/journal.sqlite3`, and bead writes in this file.
  Migrate the sanity journal into this file before the next hook is added.
- Bead creation (`bd import`) and comment ops, when the next hooks need them.
