# Phase bl design — one shared console refresh for `atm-bd cron` and `atm-bd console`

This is the only design document of phase bl. It covers the refresh track
(bl-1, bl-6, bl-7, bl-9). Paths are under
`apps/atm-bd-console/src/atm_bd_console/` in randlee/atm-bd-orchestration;
line numbers are at v0.18.4 (86853d5) and were re-checked on 2026-10-21.

## 1. Anchors (verbatim)

Rand, 2026-10-21:

> "my expectation for 'atm-bd cron' and 'atm-bd console' is that both are run on a specific phase. They both 'schedule' a info refresh which a) is exactly the same regardless of which system instigated it. b) is checked for duplication so running both does not DOUBLE the query load on bd/dolt, atm, github c) A failed run which did not produce successful data does NOT cause a run to be skipped."

Rand, 2026-10-20 (the per-control rule of the console audit):

> "if you don't have valid data, don't force an error."

Rand, 2026-10-21, relayed by the coordinator (not a quote): the plan
underestimated this; it is not "is a lock check adequate". The refresh, one
shared `runner.step` identical whichever of cron or console starts it, must
update every console control correctly, and the cron-produced result must be
what the console shows. Per control: never render from invalid or empty
input; keep last good content marked stale with age and a typed reason; a
labelled placeholder when there never was data; typed error details on screen
and in the log. Persisted last-good snapshot and per-source last-success
timestamps shared by cron and console; dedup per source (bd/dolt, atm,
github); a failed or partial run records the error, never counts as success
and never suppresses the next run; the console re-reads persisted state when
cron ran; headers and clocks must not look fresh over carried data.

## 2. What v0.18.4 does today (verified)

| Fact | Evidence |
| --- | --- |
| Cron and console are mutually exclusive, not deduplicated. Both take the one process-lifetime flock `<state_dir>/lock`; a held lock makes cron print `lock_held` and exit 0 without refreshing, and makes a second console exit 1. A running console therefore blocks every cron run for its whole life. | `state.py:139` `acquire_lock`; `cli.py:116-126`; `app.py:828-832` |
| Cron and console do not run the same refresh. Cron passes no closed-bead cache (fresh `ClosedBeads()` with no directory, re-read every run, never persisted); the console passes `ClosedBeads.load(dir)`. | `cli.py:130`, `runner.py:244`, `closed_cache.py:88`; `app.py:836-837` |
| `PollHealth` (last good data, typed failures, "since") is applied only in the console, after the step has already written `snapshot.json`. Cron never applies it; `snapshot.json` is always the raw tick, never the carried one. `PollHealth.last_good` lives in memory only, so a restart starts blank. | `runner.py:413` `_write_snapshot`; `app.py:601` `health.apply`; `poll_health.py:34,46` |
| Sanity runs, open/closed QA runs, findings and the DAG text are console attributes. Cron discards everything but the snapshot. | `cli.py:131`; `app.py:385-400,582` |
| A live console never reads what cron wrote, except `prs.json`/`merge.json`. `state` is loaded once at start and kept in memory. | `app.py:570-572,834`; `runner.py:325-326` |
| `_carry` stamps the new tick's `generated_at` on carried content, so the header clock looks fresh; only the indicator line says the data is old, and the status bar shows no carried age. | `poll_health.py:88-93`; `render.py:165-168,268-313` |
| A failed GitHub read sets `attempted_at = now` before its result is checked, so a failure suppresses the next unforced read for `github_poll_interval_s` (600 s). This breaks requirement (c). | `runner.py:334-341` |
| A failed first PR read with no cache still reports `prs_read_at = now`. | `runner.py:331,414-415` |
| Cron records interval 900 (`CRON_CADENCE_S`); the console uses `config.interval_s` (default 60). | `cli.py:26-31`, `config.py:20` |

Already fixed by v0.18.4 and **not** replanned (audit rows from the
2026-10-20 audit at 34e9066, re-checked at 86853d5): 2 beads headline, 4
errors section, 5 Held gate, 8 status table, 9 sprint list, 10 PRs tab (apart
from the `prs_read_at` defect above), 11 merge reason, 13 sanity tab layout,
15 findings pane, 16 diagnostics, 17 build line, and the duplicate-Errors
causes (a), (b), (c). Remaining rows: 1 and 12 (clock over carried data, bar
age; bl-6), 3 (agents placeholder on a first failure; bl-6), 7 (status header
prints kind+fix only, `app.py:765`; bl-7), 14 (quality "never loaded"
placeholder row, no persistence; bl-1 + bl-7).

## 3. Design

### 3.1 One entry point

New module `refresh.py`. Cron, the console tick, `atm-bd snapshot` and
`md-table` all call it; nothing else calls `runner.step`.

```python
class Source(enum.Enum):
    BD = "bd"          # phase_file, repo_config, beads.* (show_root, open, ready, show_closed, show_tasks, task_ancestors)
    ATM = "atm"        # atm.members, atm.tasks, atm.task_events
    GITHUB = "github"  # _refresh_github (PR list + CI)

@dataclass(frozen=True)
class SourceStamp:
    last_success: datetime | None   # finished_at of the last run in which every read of the source was Ok
    last_attempt: datetime | None
    last_error: Error | None        # typed error of the last failed attempt; None after a success
    since: datetime | None          # first failure of the current failing streak

@dataclass(frozen=True)
class RefreshFile:                  # <state_dir>/refresh.json, schema 1
    schema: int
    sources: Mapping[Source, SourceStamp]
    last_run: RunRecord | None      # {started_at, finished_at, by: "cron"|"console"|"snapshot", pid, outcome, sources_read}

@dataclass(frozen=True)
class Refreshed:
    result: StepResult              # the carried snapshot + sanity/quality Held values, exactly what is shown
    ran: frozenset[Source]          # sources read this call (empty: nothing was due, or another refresh holds the lock)
    skipped_reason: Literal["fresh", "in_progress"] | None

def refresh(config: Config, *, by: Literal["cron", "console", "snapshot"], force: frozenset[Source],
            clock: Callable[[], datetime], step: StepFn = runner.step) -> Refreshed: ...
```

`step` and `clock` are injected so tests drive it without real reads or
real time.

### 3.2 Locks

- `<state_dir>/refresh.lock`: `flock(LOCK_EX|LOCK_NB)` held only for the
  duration of one refresh (read, dispatch, write). A caller that cannot take
  it does not wait and does not fail: it returns the persisted result with
  `skipped_reason="in_progress"`.
- `<state_dir>/console.lock`: the console's single-instance lock (a second
  console still exits 1, as today). Cron never takes it. The old `lock` file
  is no longer used.
- Effect: a running console no longer blocks cron, and two refreshes never
  read at the same time.
- `in_progress` in `refresh.json` is informational (cron prints "refresh in
  progress by <by> pid <pid> since <t>"); the flock is the truth. A flock dies
  with its process, so a crashed refresh leaves no lock behind.

### 3.3 The dedup rule (requirements b and c)

For each source `s`, the source is **due** iff
`s in force or stamp.last_success is None or now - stamp.last_success >= window(s)`,
where `window(BD) = window(ATM) = config.interval_s` (the phase's
`console.toml` interval, read by cron and console alike; default 60) and
`window(GITHUB) = config.github_poll_interval_s` (600).

- Only `last_success` gates. A failed or partial read writes `last_attempt`,
  `last_error` and `since` and leaves `last_success` unchanged, so the next
  call is due again. This replaces `prs.json` `attempted_at` as the GitHub
  gate (`runner.py:334-341`), which today suppresses after a failure.
- A source **succeeded** in a run iff every read in its group was `Ok`.
  Partial is failure.
- If no source is due, the refresh reads nothing and returns the persisted
  result (`skipped_reason="fresh"`). Two callers at different cadences
  therefore make at most one read per source per window between them.
- If some sources are due, the step reads only those. A not-due source's
  part of the snapshot comes from the persisted last-good snapshot through
  the same carry as a failed read, but without an error.
- Dispatch (`_run_actions`) runs only in a run that read both BD and ATM
  fresh and both succeeded. A run that reused persisted BD or ATM data never
  dispatches; the run that read them already did.
- `force` (`r` key, PR-tab activation, `md-table`) marks sources due; it
  still takes `refresh.lock`.

### 3.4 One result, persisted

Inside `refresh`, under `refresh.lock`, in this order:

1. Load `state.json`, `refresh.json`, `health.json` and the persisted
   snapshot from disk (the console no longer keeps `State` in memory between
   ticks; a refresh by the other process is picked up on the next call).
2. `ClosedBeads.load(state_dir)` for every caller (cron today passes none).
3. Run the step for the due sources.
4. Apply `PollHealth` (moved out of `app.py`) to the step's snapshot. The
   carried snapshot is what is written to `snapshot.json`; `PollHealth`'s
   failures go to `health.json`, so a restart and a cron run see the same
   last-good content and the same "failed since" times.
5. Persist the console-only data the step already returns: sanity runs and
   open QA runs to `runs.json` as `Held` values (value, good_at, typed
   error), with the same keep-last-good rule.
6. Write `refresh.json` last. Every file is written temp-file + fsync +
   `os.replace` (the existing `StateStore` writer).

A crash before step 6 leaves the old stamps, so the next call re-reads (fail
open: an extra read, never a skipped one). `_run_actions` already appends
each send to `actions.jsonl` before `state.json` is written; that ordering is
unchanged.

### 3.5 Clocks never look fresh over carried data

- `Snapshot` gains `content_at: datetime`: the oldest `good_at` of any
  carried or reused part, or `generated_at` when nothing is carried.
  `generated_at` stays the tick time (it is when the refresh ran).
- The dashboard header, the Status header and the status bar print
  `content_at` and its age whenever `content_at < generated_at`, with the
  typed reason of the oldest failing source (`error_text`). `PollHealth`
  stops presenting the tick time as the data time.
- A first PR read that fails leaves `prs_read_at = None` ("not yet read"),
  never `now` (`runner.py:331`).

### 3.6 Failure and crash cases

| Case | Result |
| --- | --- |
| bd read times out | BD `last_error` set, `last_success` kept; BD due on the next call by either process; screen shows last good BD content with `content_at` and the typed error. |
| GitHub 502 | GITHUB due on the next call (no 600 s suppression). |
| Cron and console fire together | One takes `refresh.lock` and reads; the other returns the persisted result marked `in_progress`; its next tick finds the stamps fresh and reads nothing. |
| Process dies mid-refresh | flock released by the kernel; `refresh.json` unchanged; next call re-reads. |
| Hung read | Bounded by the per-command deadlines that exist today (`command.py`); while it runs, other callers get `in_progress`. Cron prints `refresh_stalled` when `in_progress.started_at` is older than `2 * window(BD)`. |
| `refresh.json` unreadable or wrong schema | Treated as "never succeeded" (every source due) and reported as a typed `IoFailed` error; never a crash, never a skip. |
| Dry runs (`snapshot`, `md-table` without `--write`) | May reuse fresh data; never write stamps. |

## 4. Test plan (deterministic; no sleeps, no threads, no real bd/atm/gh)

All tests inject `clock` and a fake `step` that returns scripted
`StepResult`s with chosen `failed_reads`; a real `tmp_path` state dir is used.

- Two callers in one process: open `refresh.lock` through two separate file
  descriptors (flock conflicts across separate opens of the same file), so
  "cron while console refreshes" is a plain sequential test.
- `test_second_caller_within_window_reads_nothing`: console at t=0 succeeds;
  cron at t=10 gets `ran == {}` and the same snapshot bytes.
- `test_failed_run_never_suppresses`: BD fails at t=0; a call at t=1 reads BD.
- `test_partial_source_is_failure`: `beads.open` Ok, `beads.ready` Err → BD
  not stamped.
- `test_github_failure_due_next_call`: GITHUB Err at t=0 → due at t=1.
- `test_cron_and_console_identical`: the same scripted reads through
  `by="cron"` and `by="console"` produce byte-identical `snapshot.json`,
  `health.json`, `runs.json`.
- `test_console_picks_up_cron_result`: cron refresh writes; the console call
  that follows returns cron's snapshot without reading.
- `test_dispatch_only_on_fresh_bd_and_atm`: a run that reuses persisted ATM
  data produces no actions.
- `test_crash_before_stamp_rereads`: the fake step raises after writing the
  snapshot; next call is due.
- `test_content_at_tracks_carried_age`: carried BD from t=0 at tick t=300 →
  header text names t=0 and "5m".
- `test_console_lock_does_not_block_cron`.

The live proof (bl-9) runs cron and console together on a real phase; see
`sprint-bl-9-refresh-integration.md`.
