# Sprint bl-8 — collector implementation and snapshot builder
Implementation repository/worktree: `randlee/atm-bd-orchestration`; branch/PR target: `integrate/phase-bl`.

closure_type: boundary; target_boundary: Collectors; branch: sprint/bl-8-collector-implementation; PR target: integrate/phase-bl; wave: 2; external_ref: #533, #492; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`CollectorRequest`, records, JSON codec); parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-6, bl-7, bl-10, bl-13.

## Scope boundary

Own `refresh.py`, `state.py`, `adapters/{bd,atm,github}.py`, `cli.py`, `command.py`, and matching collector/state tests under `apps/atm-bd-console`. This sprint alone reads sources. Every adapter, persisted record, lock, and snapshot is production-ready at this boundary.

## Governing text (verbatim)

> "we do NOT need to collect data from all sources at the same rate."

> "my expectation for 'atm-bd cron' and 'atm-bd console' is that both are run on a specific phase. They both 'schedule' a info refresh which a) is exactly the same regardless of which system instigated it. b) is checked for duplication so running both does not DOUBLE the query load on bd/dolt, atm, github c) A failed run which did not produce successful data does NOT cause a run to be skipped."

## Authoritative deliverables

1. Implement sole source reads in `adapters/{__init__,bd,atm,github}.py`, each adapter being a separate module that owns its own rate limit, timeout and failure handling (bd/dolt timeout, ATM no limit, GitHub quota); this phase has no git collector (no git window); with atomic persisted JSON `CollectorRecord`s and short nonblocking per-source locks. Due windows are BD/dolt 60 seconds, ATM 60 seconds, GitHub 600 seconds (the 10-minute default); only successful data advances `last_success`. The GitHub window is configurable in repository configuration and on the command line; `github.py` reads live quota via `gh api rate_limit` before each refresh, skips below the configured threshold (default 50 percent) keeping cached data with a paused status carrying remaining, limit and local reset time (resource graphql or core by mode), a paused skip never advances `last_success`, fetches up to 100 PRs per GraphQL page with few fields and pages past 100 without dropping PRs.
2. Implement `build_snapshot(records, clock, derive_problem_records) -> StateSnapshot` in `state.py`. It calls bl-10's pure fixture/codec-backed `derive_problem_records` seam, carries last-good data and typed reason, derives oldest valid `content_at`, and labels never-read state. It reads only persisted records and writes only the `StateSnapshot`; collection writes only `CollectorRecord`s and event firing is the later bl-3 stage, so no call performs more than one stage; `build_snapshot` raises no event request because a run read fresh data.
3. Implement `r` as a force request: it bypasses only due eligibility (never the quota pause: a forced refresh is refused while below the threshold), respects an existing source lock and consumes its result without a second query; success restarts only that source timer and failure remains due; a failed or partial read leaves that source due on the next request.
4. A failed read is logged once when it happens; a cached failure carried into the snapshot (such as the last GitHub error kept with cached PR data) is shown only as a status and never logged again per tick or per `build_snapshot` call.
5. When cron skips collection because no source is due, `cli.py` prints the same status information built from the persisted snapshot as when it collected.
6. Keep D1 deadline input gated in `command.py`: only the Rand-supplied D1 value permits command dispatch; D2/#492 remains open and no value is invented.

## Authoritative acceptance criteria

- [ ] `test_collector_windows_are_source_specific`
- [ ] `test_second_caller_within_window_reads_nothing`
- [ ] `test_refresh_lock_returns_in_progress_without_second_query`
- [ ] `test_failed_or_partial_read_is_due_on_next_request`
- [ ] `test_force_request_respects_lock_and_success_only_resets_own_timer`
- [ ] `test_snapshot_from_failed_and_superseded_fixture_carries_expected_problems`
- [ ] `test_build_snapshot_content_at_carried_reason_and_never_read`
- [ ] `test_d1_deadline_is_required_before_command_dispatch`
- [ ] `test_command_and_each_adapter_rejects_missing_or_non_d1_deadline`
- [ ] `test_atomic_record_and_snapshot_persistence`
- [ ] `test_atomic_record_replacement_returns_last_good_snapshot`
- [ ] `test_each_call_performs_one_stage_only`
- [ ] `test_github_window_configurable_quota_pause_and_paging_past_100`
- [ ] `test_cron_skipped_collection_prints_same_status_from_snapshot`
- [ ] `test_ten_ticks_with_one_cached_github_error_log_one_entry`

Tests inject clocks, storage, source responses, locks, and the derivation function; no test uses sleep, scheduling, load, or a live source.

## Authoritative validation
```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q tests/test_refresh.py tests/test_state.py
```

## Non-closure

It does not select checks or derive problems (bl-10), queue/deliver events (bl-3), render UI, or prove end-to-end cron/TUI equality (bl-12).
