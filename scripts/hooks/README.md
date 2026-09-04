# ATM lifecycle hooks

`claude_queue_hook.py` and `codex_queue_hook.py` are thin adapters over
`atm_queue_hook.py`. A Stop queue pull is fail-closed for diagnostics: missing
caller context, an unavailable daemon, or an ATM CLI failure returns non-zero
and writes the reason to stderr instead of silently allowing a harness Stop to
pass. Set `ATM_BIN` when the `atm` executable is not on `PATH`.

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

The hook presents caller context already held in `ATM_TEAM` and
`ATM_IDENTITY`, and requires an absolute existing `ATM_HOME` (or `HOME`) for a
Stop pull; it has no target-member option. The Stop call passes `--team`,
`--as`, and the hidden `--require-daemon` flag explicitly to the internal CLI.
Use
`python3 scripts/hooks/test_queue_hooks.py` for the deterministic fake-CLI
contract tests.

## Installation

Install the reference scripts in a checkout of `atm-core`, then replace
`/absolute/path/to/atm-core` in the snippets below with that checkout's
absolute path. Keep the existing top-level `hooks` entries in each file and
merge these event entries into them.

For Claude Code, add the following to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 /absolute/path/to/atm-core/scripts/hooks/atm_queue_hook.py --event pre-tool-use --harness claude"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 /absolute/path/to/atm-core/scripts/hooks/atm_queue_hook.py --event stop --harness claude"
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 /absolute/path/to/atm-core/scripts/hooks/atm_queue_hook.py --event session-end --harness claude"
          }
        ]
      }
    ]
  }
}
```

For Codex, add the following to `~/.codex/hooks.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 /absolute/path/to/atm-core/scripts/hooks/atm_queue_hook.py --event pre-tool-use --harness codex"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 /absolute/path/to/atm-core/scripts/hooks/atm_queue_hook.py --event stop --harness codex"
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 /absolute/path/to/atm-core/scripts/hooks/atm_queue_hook.py --event session-end --harness codex"
          }
        ]
      }
    ]
  }
}
```

The hook environment must provide `ATM_TEAM`, `ATM_IDENTITY`, and an absolute
existing `ATM_HOME` (or `HOME`) for Stop queue pulls. `ATM_BIN` overrides the
`atm` executable; `ATM_HOOK_STATE_DIR`, `ATM_HOOK_DEBOUNCE_SECONDS`, and
`ATM_HOOK_TIMEOUT_SECONDS` make state and timing deterministic in tests. The
raw `_internal-queue-get` CLI is fail-open when the daemon is unavailable, but
the lifecycle Stop hook uses `--require-daemon` and reports missing context or
diagnostic failure on stderr with a non-zero status.

These Python reference scripts are the MVP contract for the later schook Rust
plugin, which will link `atm-core` as a library. The schook plugin is out of
scope for this sprint.
