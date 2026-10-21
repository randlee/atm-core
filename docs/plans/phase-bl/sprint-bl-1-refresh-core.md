# Sprint bl-1 — shared refresh core: one refresh for cron and console, per-source success stamps, persisted last-good

closure_type `contract`; target_boundary `BOUNDARY-Refresh` (`refresh.refresh`,
`refresh.json`, `health.json`, `runs.json`, the carried `snapshot.json`).
Branch `sprint/bl-1-refresh-core`, PR → `integrate/phase-bl` (layer 1). Wave 1; no
dependency. Priority P1 (the TUI showing improper information).
`external_ref`: `gh-126`.
recommended_agent `arch-ctm`; recommended_model `opus`.

Authority: `design.md` (this directory) §1-§4; the anchors there are Rand's
verbatim text and the coordinator's relayed correction. Paths under
`apps/atm-bd-console/src/atm_bd_console/`; line numbers at v0.18.4 (86853d5).

## Scope boundary

In: everything in `design.md` §3 that lives below the UI: `refresh.py`,
locks, stamps and the dedup rule, `PollHealth` moved into the refresh and
persisted, sanity/QA run data persisted, `content_at`, the GitHub
`attempted_at` defect (`runner.py:334-341`), the `prs_read_at` defect
(`runner.py:331`), cron's closed-bead cache and interval, and #126 item 2b's
task re-read before `atm task assign`. Cron (`cli.py`) is switched to
`refresh.refresh` here.

Out: `app.py` (bl-7 switches the console tick to `refresh.refresh` and
renders the persisted values), `render.py`/`model.py` (bl-6 renders
`content_at`), the live proof (bl-9). Until bl-7 lands the console keeps
working on the old path; `runner.step`'s positional signature is unchanged
and new parameters are keyword-only with defaults that keep today's
behaviour, so bl-3's `test_dispatch_tick.py` stays valid.

## Owned paths

- new `refresh.py`; edited `state.py`, `poll_health.py`, `runner.py`,
  `step_result.py`, `cli.py`, `types.py` (`Snapshot.content_at`), `codec.py`
  (its encoding), `closed_cache.py` (only if `load` needs the new state dir
  layout)
- tests: new `tests/test_refresh.py`; edited `tests/test_state.py`,
  `tests/test_poll_health.py`, `tests/test_runner.py`, `tests/test_cli.py`,
  `tests/test_codec.py`, `tests/test_types.py`, `tests/test_closed_cache.py`
- `plugins/atm-bd-orchestration/skills/atm-bd-orchestration/SKILL.md`: only
  the paragraph describing `atm-bd cron` and the console lock (no new plugin
  file; `manifest.yaml` untouched)

## Public contract

As written in `design.md` §3.1 (`Source`, `SourceStamp`, `RefreshFile`,
`Refreshed`, `refresh(config, *, by, force, clock, step)`), plus:

```python
# state.py
def acquire_console_lock(repo_root, root_id) -> Result[ConsoleLock]: ...   # <state_dir>/console.lock, process lifetime
def try_refresh_lock(directory: Path) -> Result[RefreshLock | None]: ...  # None: held by another refresh
def load_refresh(directory) -> Result[RefreshFile]: ...; def write_refresh(directory, f: RefreshFile) -> Result[None]: ...
def load_health(directory) -> Result[HealthFile]: ...;  def write_health(...)
def load_runs(directory) -> Result[RunsFile]: ...;      def write_runs(...)   # Held sanity + open QA runs
# poll_health.py
class PollHealth:
    @classmethod
    def restore(cls, health: HealthFile, last_good: Snapshot | None) -> PollHealth: ...
    def to_file(self) -> HealthFile: ...
    def apply(self, snapshot: Snapshot, failed: Mapping[str, Error], reused: frozenset[str] = frozenset()) -> Snapshot: ...
        # reused: sources not due this run, carried from last_good without an error
# types.py
Snapshot.content_at: datetime   # oldest good_at of any carried/reused part; == generated_at when none
```

## Authoritative deliverables

1. **`refresh.py`** implementing `design.md` §3.1-§3.4 exactly: locks, the
   per-source due rule (only `last_success` gates; partial = failure),
   reading only due sources, carrying reused ones, dispatch only on fresh
   successful BD and ATM, `PollHealth` applied inside the refresh, all files
   written temp + fsync + `os.replace`, `refresh.json` last.
2. **Locks.** `console.lock` for the console instance; `refresh.lock` per
   refresh; the old `lock` file retired. A held `refresh.lock` returns the
   persisted result with `skipped_reason="in_progress"`.
3. **Cron = console.** `cli.py` cron calls `refresh.refresh(by="cron")` with
   `ClosedBeads.load(state_dir)` (today none, `cli.py:130`) and the phase's
   `config.interval_s` as the dedup window (the 900 s `CRON_CADENCE_S` stays
   only as cron's own schedule hint). Cron prints the refresh outcome, the
   sources read, and `refresh in progress` / `refresh_stalled` per §3.6.
   `atm-bd snapshot` and `md-table` call `refresh.refresh(by="snapshot")` and
   never write stamps in their dry modes.
4. **Persisted last-good.** `snapshot.json` is the carried snapshot;
   `health.json` holds `PollHealth` failures; `runs.json` holds sanity and
   open-QA `Held` values (`held.py`), so cron produces them too.
5. **GitHub gate.** The GitHub source uses the stamp (`last_success`), not
   `prs.json` `attempted_at` (`runner.py:334-341`); a failed read is due on
   the next call. A failed first read leaves `prs_read_at = None`
   (`runner.py:331`).
6. **`content_at`** set by `PollHealth.apply` (design §3.5).
7. **#126 2b re-read.** In `runner._run_actions`, immediately before each
   `atm_source.assign`, re-read the bead's ATM task state with the existing
   task read; if a task now exists, write a `hold` action line naming the
   task id instead of assigning. (The bead-assignee write in the same #126
   item is D4.)

## Authoritative acceptance criteria (deterministic; injected clock and fake step)

gh-126 refresh requirement (a) identical
- [ ] `test_cron_and_console_identical`: the same scripted reads through `by="cron"` and `by="console"` produce byte-identical `snapshot.json`, `health.json` and `runs.json`.
- [ ] `test_cron_loads_closed_cache`: cron passes a `ClosedBeads` whose directory is the state dir.

(b) no doubled load
- [ ] `test_second_caller_within_window_reads_nothing`: console succeeds at t=0, cron at t=10 → `ran == frozenset()` and the fake step was not called.
- [ ] `test_refresh_lock_held_returns_in_progress`: a second descriptor holding `refresh.lock` → `skipped_reason == "in_progress"`, no step call, no exception.
- [ ] `test_console_lock_does_not_block_cron`.

(c) a failed run never suppresses
- [ ] `test_failed_run_never_suppresses`: BD fails at t=0 → a call at t=1 reads BD; `last_success` unchanged, `last_error` typed.
- [ ] `test_partial_source_is_failure`: `beads.open` Ok, `beads.ready` Err → BD not stamped.
- [ ] `test_github_failure_due_next_call`: GITHUB Err at t=0 → due at t=1 (today suppressed 600 s).
- [ ] `test_crash_before_stamp_rereads`: the fake step raises after the snapshot write → next call reads.

Persisted / dispatch / clock
- [ ] `test_console_picks_up_cron_result`: after a cron refresh, a console call returns cron's snapshot without reading.
- [ ] `test_dispatch_only_on_fresh_bd_and_atm`: a run reusing ATM data yields no actions.
- [ ] `test_health_survives_restart`: `PollHealth.restore(to_file())` gives the same `indicator()` text.
- [ ] `test_content_at_tracks_carried_age`: BD carried from t=0 at tick t=300 → `content_at == t0`.
- [ ] `test_failed_first_pr_read_not_read_at_now`: `prs_read_at is None`.
- [ ] `test_assign_skipped_when_task_appeared` (#126 2b): the re-read finds a task → a `hold` line with that task id and no assign call.

## Authoritative validation

```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
cd plugins/atm-bd-orchestration && python3 tests/gen_manifest.py --check && python3 -m pytest -q -rs
```

No sleeps, no threads, no real bd/atm/gh: two `refresh.lock` descriptors in
one process model the second process (flock conflicts across separate opens).

## Non-closure

#126 stays open: the console adoption (bl-7), the live proof (bl-9) and the
items in D4 remain.
