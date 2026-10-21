# Sprint bl-8 — collector implementation and snapshot builder

closure_type: boundary; target_boundary: Collectors; branch:
sprint/bl-8-collector-implementation; PR target: integrate/phase-bl; wave 2;
external_ref: #533,#492; recommended_agent: trinity; recommended_model: gpt-6.1-sol.
must_follow: bl-1 (PhaseBlContract). parallel_safe: bl-2,3,4,5,6,7,10,13,14.

Own refresh.py, state.py, adapters/bd.py, adapters/atm.py, adapters/github.py,
cli.py, command.py and matching named test files under apps/atm-bd-console.
Every deliverable is production-ready in this boundary.

1. Implement each source collector in its sole adapter file with the bl-1
lock/due contract and atomic JSON record persistence.
2. state.py owns pure build_snapshot(records, clock) -> StateSnapshot and
persists current state; it computes oldest good content_at, preserves carried
last-good data/typed reason, and labels never-read state.
3. D1-gated deadline seam: command.py accepts the Rand-supplied D1 value from
team-lead; until supplied dispatch blocks and invents no deadline. D2/#492 is
an explicit carry-forward awaiting its ruling.

- [ ] test_each_adapter_has_one_controlled_source_and_test_file
- [ ] test_build_snapshot_content_at_carried_reason_and_never_read
- [ ] test_d1_deadline_is_required_before_command_dispatch
- [ ] test_atomic_record_and_snapshot_persistence

No send, debounce, display, or composition proof closes here; bl-12 owns
feature-level cron/TUI equality and retry evidence.
