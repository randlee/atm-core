# ATM lifecycle hooks

`claude_queue_hook.py` and `codex_queue_hook.py` are thin adapters over
`atm_queue_hook.py`. They are intentionally fail-open: a missing or unreachable
daemon never blocks the host harness. Set `ATM_BIN` when the `atm` executable is
not on `PATH`.

Supported events are `pre-tool-use`, `stop`, and `session-end`. A Stop performs
one synchronous `_internal-queue-get`; Claude prints the literal JSON block
decision when messages are returned, while Codex consumes the same pull without
printing a harness-specific decision. Stop also starts a detached, debounced
idle timer. A subsequent PreToolUse cancels the timer; expiry sends exactly one
`_internal-heartbeat --activity idle`.

The state directory and timing are operator-testable:

```text
ATM_HOOK_STATE_DIR=.tmp/atm-hooks
ATM_HOOK_DEBOUNCE_SECONDS=2
ATM_HOOK_TIMEOUT_SECONDS=2
ATM_BIN=/path/to/atm
```

The hook only presents caller context already held in `ATM_TEAM` and
`ATM_IDENTITY`; it has no target-member option. Use
`python3 scripts/hooks/test_queue_hooks.py` for the deterministic fake-CLI
contract tests.
