# Smoke Thorough

- status: `failed`
- timestamp: `2026-08-08T03:03:50.446242+00:00`
- binary SHA: `be5e16ec8285fd999bf81dc150439207401518b7`
- duration secs: `36.451`
- summary: `pass=28`, `fail=4`, `skip=0`
- row semantics: `PASS` means every command in the row exited `0`; `FAIL`
  records the first failing command only and does not claim sibling commands in
  that row were executed after the failure

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `AD11-CMD-SEND-001` | send command preserves caller-context ownership across environment and explicit override paths | `PASS` | send stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-READ-001` | read command preserves caller-context ownership across environment and explicit override paths | `PASS` | read stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-ACK-001` | ack command preserves caller-context ownership across environment and explicit override paths | `PASS` | ack stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-LIST-001` | list command preserves retained filters while keeping caller-context ownership explicit | `PASS` | list preserves retained filters while staying bound to explicit caller-context ownership |
| `AD11-CMD-CLEAR-001` | clear command preserves caller-context ownership across environment and explicit override paths | `PASS` | clear stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-LOG-001` | log command remains daemon-independent with caller-context enforcement at the CLI boundary | `PASS` | log remains daemon-independent and still fails locally when caller context is unavailable |
| `AD11-CMD-MEMBERS-001` | members command remains daemon-independent while preserving explicit team override handling | `PASS` | members remains daemon-independent while preserving retained caller-team semantics |
| `AD11-CMD-TEAMS-001` | teams list command remains daemon-independent on the retained CLI surface | `PASS` | teams list remains daemon-independent on the retained CLI surface |
| `AD11-CMD-TEAMS-ADD-MEMBER-001` | teams add-member preserves the retained home-dir payload contract | `PASS` | teams add-member preserves the retained home-dir payload contract |
| `AD11-CMD-TEAMS-UPDATE-MEMBER-001` | teams update-member preserves caller context and fails locally when mandatory caller context is missing | `PASS` | teams update-member preserves caller context and still fails locally when mandatory caller context is missing |
| `AD11-CMD-TEAMS-BACKUP-001` | teams backup preserves retained team scoping and remains daemon-independent in dry-run execution | `PASS` | teams backup preserves retained team scoping and remains daemon-independent in dry-run execution |
| `AD11-CMD-TEAMS-RESTORE-001` | teams restore preserves retained path and dry-run behavior without requiring the daemon | `PASS` | teams restore preserves retained path and dry-run behavior without requiring the daemon |
| `AD11-CMD-DOCTOR-001` | doctor remains identity-free while preserving optional team scoping and the direct local path | `PASS` | doctor remains identity-free while preserving optional team scoping and the direct local path |
| `AD11-POSTSEND-LOCAL-TMUX-001` | local tmux post-send requires and uses authoritative pane metadata | `PASS` | local tmux post-send remains bound to authoritative roster pane metadata |
| `AD11-POSTSEND-WARNING-001` | sender-visible warning fallback survives failed post-send emission | `PASS` | failed post-send emission still degrades into a sender-visible warning after durable send success |
| `AD11-ROSTER-REPAIR-001` | fixture evidence preserves repaired pane metadata through team-admin and doctor projections | `PASS` | fixture-backed smoke evidence proves pane repair survives the accepted team-admin and doctor projection paths |
| `AD11-XREPO-001` | sender roster home_dir governs post-send config lookup across repos | `PASS` | post-send config discovery remains anchored to sender roster metadata rather than ambient caller cwd, preserving cross-repo local-send behavior |
| `AD11-GRAFT-001` | graft-backed post-send uses a direct same-host receiver socket with typed warning fallback | `FAIL` | first failing command: cargo test -p atm-daemon tests_post_send_graft_warning::dispatcher_send_delivers_direct_graft_nudge_without_warning -- --exact |
| `GRAFT-001` | real same-host atm-graft host registers, consumes an advisory nudge, and completes unary read/ack/send on the shared daemon contract | `FAIL` | first failing command: /opt/homebrew/opt/python@3.11/bin/python3.11 scripts/smoke/run_graft_same_host.py |
| `AD11-AUTH-001` | update-member auth checks and infallible add-member projection are closed | `PASS` | the promoted AD.9 auth and infallible findings are closed: update-member consumes caller context materially, and add-member projection no longer pretends to fail |
| `AD11-READINESS-001` | phase-ad readiness and boundary artifacts fail closed | `PASS` | Phase AD readiness records, smoke artifacts, and MessageReceivedHookEmitter boundary inventory are all present and wired into the retained validation gate |
| `AD17-ULID-001` | retained ATM message identity stays ULID-only on the accepted line | `PASS` | ULID-only message identity remains enforced in the retained SQLite mailbox state |
| `AD17-READ-001` | read mutation and contains filtering stay self-consistent on the durable store-backed path | `PASS` | read mutation still reports the post-mutation state and contains filtering still sees the durable full-body projection |
| `AD17-CI-001` | windows CI retains the explicit atm-daemon lane on the accepted line | `FAIL` | first failing command: rg -n 'Run atm-daemon tests' .github/workflows/ci.yml |
| `AD18-RUNTIME-ROOT-001` | shared-host raw CLI bootstrap reuses a single daemon and keeps runtime state under the accepted ATM_HOME root | `FAIL` | first failing command: /opt/homebrew/opt/python@3.11/bin/python3.11 scripts/smoke/run_thorough_shared_host.py |
| `AD19-READ-OUTPUT-001` | read mutation returns the message it actually mutated together with post-mutation bucket counts | `PASS` | read returns the durable message it actually mutated, reports post-mutation unread counts, and leaves ack mutation semantics intact |
| `AD20-READ-CONTAINS-001` | metadata-backed contains stays full-body correct while keeping durable-body reload bounded | `PASS` | metadata-backed contains stays full-body correct and only reloads durable body for surviving summary-miss candidates |
| `AD29-POSTSEND-EXTERNAL-001` | external post-send hook success suppresses built-in fallback while preserving durable send success | `PASS` | external post-send hook success keeps the built-in nudge path inactive while durable send success remains intact |
| `AD29-POSTSEND-PARTIAL-001` | mixed post-send hook outcomes preserve durable delivery while surfacing sender-visible warnings | `PASS` | mixed hook accounting preserves durable delivery success and retains a sender-visible warning for failed matches |
| `AD29-POSTSEND-BUILTIN-001` | built-in fallback covers both tmux and graft recipients when no external hook matches | `PASS` | built-in fallback stays honest for both tmux-backed and graft-backed recipients when no external hook matches |
| `AD29-POSTSEND-RESET-001` | deleting a prior override row restores the built-in default template path | `PASS` | removing a stored override row re-exposes the built-in default template instead of leaving an implicit disabled state behind |
| `AD29-POSTSEND-DISABLE-001` | explicitly disabled built-in template state skips local post-send delivery cleanly | `PASS` | the explicit disabled-template state becomes a documented no-delivery path instead of an accidental empty-string side effect |


## Deviations

### `AD11-GRAFT-001`

- observed: {
  "command": [
    "cargo",
    "test",
    "-p",
    "atm-daemon",
    "tests_post_send_graft_warning::dispatcher_send_delivers_direct_graft_nudge_without_warning",
    "--",
    "--exact"
  ],
  "exit_code": 101,
  "stdout": "",
  "stderr": "Compiling tracing-core v0.1.36\n   Compiling log v0.4.33\n   Compiling sc-observability v1.2.0\n   Compiling serial_test v3.5.0\n   Compiling tracing v0.1.44\n   Compiling tower v0.5.3\n   Compiling hyper-util v0.1.20\n   Compiling agent-team-mail-core v1.4.1-beta-ai-1 (/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/crates/atm-core)\n   Compiling atm-storage-rusqlite v1.4.1-beta-ai-1 (/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/crates/atm-storage-rusqlite)\n   Compiling tower-http v0.6.11\n   Compiling hyper-rustls v0.27.9\n   Compiling axum v0.8.9\n   Compiling reqwest v0.12.28\n   Compiling atm-http-runtime v1.4.1-beta-ai-1 (/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/crates/atm-http-runtime)\n   Compiling atm-runtime v1.4.1-beta-ai-1 (/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/crates/atm-runtime)\n   Compiling atm-daemon-bootstrap v1.4.1-beta-ai-1 (/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/crates/atm-daemon-bootstrap)\n   Compiling atm-daemon v1.4.1-beta-ai-1 (/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/crates/atm-daemon)\nerror[E0432]: unresolved import `atm_core::test_support`\n    --> crates/atm-daemon/src/../bin_support/daemon_observability.rs:1072:19\n     |\n1072 |     use atm_core::test_support::EnvGuard;\n     |                   ^^^^^^^^^^^^ could not find `test_support` in `atm_core`\n     |\nnote: found an item that was configured out\n    --> crates/atm-core/src/lib.rs:79:9\n     |\n  78 | #[cfg(any(test, feature = \"test-utils\"))]\n     |          ------------------------------ the item is gated here\n  79 | pub mod test_support;\n     |         ^^^^^^^^^^^^\n\nFor more information about this error, try `rustc --explain E0432`.\nerror: could not compile `atm-daemon` (bin \"atm-daemon\" test) due to 1 previous error"
}
- expected: all targeted validation commands exit 0
- likely root cause: one or more targeted Phase AD evidence checks regressed
- artifact: cargo test -p atm-daemon tests_post_send_graft_warning::dispatcher_send_delivers_direct_graft_nudge_without_warning -- --exact

### `GRAFT-001`

- observed: {
  "command": [
    "/opt/homebrew/opt/python@3.11/bin/python3.11",
    "scripts/smoke/run_graft_same_host.py"
  ],
  "exit_code": 1,
  "stdout": "",
  "stderr": "Traceback (most recent call last):\n  File \"/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/scripts/smoke/run_graft_same_host.py\", line 411, in <module>\n    raise SystemExit(main())\n                     ^^^^^^\n  File \"/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/scripts/smoke/run_graft_same_host.py\", line 224, in main\n    daemon_pids_before = isolated_daemon_baseline()\n                         ^^^^^^^^^^^^^^^^^^^^^^^^^^\n  File \"/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/scripts/smoke/run_graft_same_host.py\", line 165, in isolated_daemon_baseline\n    require_clean_host_daemon_state(smoke_label=\"graft same-host smoke\")\n  File \"/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/scripts/smoke/daemon_lifecycle.py\", line 122, in require_clean_host_daemon_state\n    raise RuntimeError(\nRuntimeError: graft same-host smoke requires an isolated OS user with no existing atm-daemon; refusing to attach to or terminate an ambient daemon"
}
- expected: all targeted validation commands exit 0
- likely root cause: one or more targeted Phase AD evidence checks regressed
- artifact: /opt/homebrew/opt/python@3.11/bin/python3.11 scripts/smoke/run_graft_same_host.py

### `AD17-CI-001`

- observed: {
  "command": [
    "rg",
    "-n",
    "Run atm-daemon tests",
    ".github/workflows/ci.yml"
  ],
  "exit_code": 1,
  "stdout": "",
  "stderr": ""
}
- expected: all targeted validation commands exit 0
- likely root cause: one or more targeted Phase AD evidence checks regressed
- artifact: rg -n 'Run atm-daemon tests' .github/workflows/ci.yml

### `AD18-RUNTIME-ROOT-001`

- observed: {
  "command": [
    "/opt/homebrew/opt/python@3.11/bin/python3.11",
    "scripts/smoke/run_thorough_shared_host.py"
  ],
  "exit_code": 1,
  "stdout": "",
  "stderr": "Traceback (most recent call last):\n  File \"/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/scripts/smoke/run_thorough_shared_host.py\", line 524, in <module>\n    raise SystemExit(main())\n                     ^^^^^^\n  File \"/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/scripts/smoke/run_thorough_shared_host.py\", line 206, in main\n    require_clean_host_daemon_state(smoke_label=\"shared-host smoke\")\n  File \"/Users/randlee/Documents/github/atm-core-worktrees/feature/pal-s11-m5-hardware-smoke-dispatch/scripts/smoke/daemon_lifecycle.py\", line 122, in require_clean_host_daemon_state\n    raise RuntimeError(\nRuntimeError: shared-host smoke requires an isolated OS user with no existing atm-daemon; refusing to attach to or terminate an ambient daemon"
}
- expected: all targeted validation commands exit 0
- likely root cause: one or more targeted Phase AD evidence checks regressed
- artifact: /opt/homebrew/opt/python@3.11/bin/python3.11 scripts/smoke/run_thorough_shared_host.py
