# Smoke Thorough

- status: `failed`
- timestamp: `2026-07-13T06:08:28.613356+00:00`
- binary SHA: `ef7e26a1bf242f9b57fc96018f51d2d491cdbb4a`
- duration secs: `975.478`
- summary: `pass=30`, `fail=1`, `skip=0`
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
| `AD11-CMD-DOCTOR-001` | doctor remains identity-free while preserving optional team scoping and the direct local path | `FAIL` | first failing command: cargo test -p agent-team-mail commands::doctor::tests::execute_runs_direct_local_doctor_path -- --exact |
| `AD11-POSTSEND-LOCAL-TMUX-001` | local tmux post-send requires and uses authoritative pane metadata | `PASS` | local tmux post-send remains bound to authoritative roster pane metadata |
| `AD11-POSTSEND-WARNING-001` | sender-visible warning fallback survives failed post-send emission | `PASS` | failed post-send emission still degrades into a sender-visible warning after durable send success |
| `AD11-ROSTER-REPAIR-001` | fixture evidence preserves repaired pane metadata through team-admin and doctor projections | `PASS` | fixture-backed smoke evidence proves pane repair survives the accepted team-admin and doctor projection paths |
| `AD11-XREPO-001` | sender roster home_dir governs post-send config lookup across repos | `PASS` | post-send config discovery remains anchored to sender roster metadata rather than ambient caller cwd, preserving cross-repo local-send behavior |
| `AD11-GRAFT-001` | graft-backed post-send uses a direct same-host receiver socket with typed warning fallback | `PASS` | the graft-backed emission seam performs one bounded same-host receiver delivery attempt and still surfaces typed sender warnings when the receiver path is unavailable |
| `AD11-AUTH-001` | update-member auth checks and infallible add-member projection are closed | `PASS` | the promoted AD.9 auth and infallible findings are closed: update-member consumes caller context materially, and add-member projection no longer pretends to fail |
| `AD11-READINESS-001` | phase-ad readiness and boundary artifacts fail closed | `PASS` | Phase AD readiness records, smoke artifacts, and PostSendHookEmitter boundary inventory are all present and wired into the retained validation gate |
| `AD17-ULID-001` | retained ATM message identity stays ULID-only on the accepted line | `PASS` | ULID-only message identity remains enforced in retained schema and workflow state |
| `AD17-READ-001` | read mutation and contains filtering stay self-consistent on the durable store-backed path | `PASS` | read mutation still reports the post-mutation state and contains filtering still sees the durable full-body projection |
| `AD17-CI-001` | windows CI retains the explicit atm-daemon lane on the accepted line | `PASS` | the explicit atm-daemon CI lane remains present and the Windows skip guard is absent |
| `AD18-RUNTIME-ROOT-001` | shared-host raw CLI bootstrap reuses a single daemon and keeps runtime state under the accepted ATM_HOME root | `PASS` | shared-host smoke proves multi-workspace raw CLI bootstrap reuses one daemon, preserves team isolation, and keeps runtime ownership under the accepted ATM_HOME root |
| `AD19-READ-OUTPUT-001` | read mutation returns the message it actually mutated together with post-mutation bucket counts | `PASS` | read returns the durable message it actually mutated, reports post-mutation unread counts, and leaves ack mutation semantics intact |
| `AD20-READ-CONTAINS-001` | metadata-backed contains stays full-body correct while keeping durable-body reload bounded | `PASS` | metadata-backed contains stays full-body correct and only reloads durable body for surviving summary-miss candidates |
| `AD29-POSTSEND-EXTERNAL-001` | external post-send hook success suppresses built-in fallback while preserving durable send success | `PASS` | external post-send hook success keeps the built-in nudge path inactive while durable send success remains intact |
| `AD29-POSTSEND-PARTIAL-001` | mixed post-send hook outcomes preserve durable delivery while surfacing sender-visible warnings | `PASS` | mixed hook accounting preserves durable delivery success and retains a sender-visible warning for failed matches |
| `AD29-POSTSEND-BUILTIN-001` | built-in fallback covers both tmux and graft recipients when no external hook matches | `PASS` | built-in fallback stays honest for both tmux-backed and graft-backed recipients when no external hook matches |
| `AD29-POSTSEND-RESET-001` | deleting a prior override row restores the built-in default template path | `PASS` | removing a stored override row re-exposes the built-in default template instead of leaving an implicit disabled state behind |
| `AD29-POSTSEND-DISABLE-001` | explicitly disabled built-in template state skips local post-send delivery cleanly | `PASS` | the explicit disabled-template state becomes a documented no-delivery path instead of an accidental empty-string side effect |


## Deviations

### `AD11-CMD-DOCTOR-001`

- observed: {
  "command": [
    "cargo",
    "test",
    "-p",
    "agent-team-mail",
    "commands::doctor::tests::execute_runs_direct_local_doctor_path",
    "--",
    "--exact"
  ],
  "exit_code": 101,
  "stdout": "running 1 test\ntest commands::doctor::tests::execute_runs_direct_local_doctor_path ... FAILED\n\nfailures:\n\n---- commands::doctor::tests::execute_runs_direct_local_doctor_path stdout ----\n\nthread 'commands::doctor::tests::execute_runs_direct_local_doctor_path' (5082077) panicked at crates/atm/src/commands/doctor.rs:163:9:\nassertion failed: report.daemon_runtime.is_none()\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\n\nfailures:\n    commands::doctor::tests::execute_runs_direct_local_doctor_path\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 129 filtered out; finished in 0.04s",
  "stderr": "Finished `test` profile [unoptimized + debuginfo] target(s) in 0.06s\n     Running unittests src/main.rs (target/debug/deps/atm-fa0acaa079844942)\nerror: test failed, to rerun pass `-p agent-team-mail --bin atm`"
}
- expected: all targeted validation commands exit 0
- likely root cause: one or more targeted Phase AD evidence checks regressed
- artifact: cargo test -p agent-team-mail commands::doctor::tests::execute_runs_direct_local_doctor_path -- --exact

## 1.3.0 Dogfood Findings

### `SMOKE-FIND-001` — host-wide daemon singleton is bypassable with `ATM_HOME`

- severity: `release-blocking`
- observed: Three 1.3.0 `atm-daemon` processes were simultaneously live on the
  same macOS host. One owned `/Users/randlee/.atm/daemon/owner.lock`; two smoke
  fixture daemons owned separate `owner.lock` files beneath distinct temporary
  `ATM_HOME` roots.
- expected: ADR-002 requires at most one `atm-daemon` anywhere on the host. It
  expressly rejects alternate `ATM_HOME` values and test exceptions.
- root cause: the client launch gate calls
  `atm_core::home::host_runtime_lock_path(HOST_RUNTIME_LAUNCH_LOCK_FILE)`
  (`crates/atm-daemon-client/src/lib.rs:650`), and the daemon owner gate calls
  the same `ATM_HOME`-derived helper (`crates/atm-daemon/src/host_ownership.rs:80`).
  `host_runtime_lock_path*` resolves beneath `<ATM_HOME>/.atm/daemon`, so a
  second `ATM_HOME` creates distinct launch and owner locks as well as a
  distinct socket namespace. The test
  `launch_gate_isolated_per_atm_home_root` in `crates/atm/src/composition.rs`
  explicitly asserts that this bypass is intended.
- broken layers: the pre-spawn file lock and daemon-side ownership file lock
  are both per-`ATM_HOME`; the different per-home socket paths do not conflict;
  the static smoke/lint gate permits `run_thorough_shared_host.py` to spawn
  fixture daemons with temporary homes.
- required 1.3.1 fix: introduce one stable OS-user/host runtime root that does
  not consult `ATM_HOME`; use it for launch lock, owner lock, and singleton
  endpoint admission. Reject a second startup before binding any endpoint.
  Replace the per-`ATM_HOME` acceptance test with a process-level test that
  launches a first daemon under home A and proves launch under home B exits
  with `ATM_DAEMON_LAUNCH_GATE_REJECTED` or
  `ATM_DAEMON_SERVING_STATE_REJECTED`. There is no test-only exemption:
  smoke fixtures and every other launcher must traverse the production gate;
  the only permitted second-launch result is typed rejection, with no daemon
  process left behind.
- recovery performed: force-stopped the two temporary fixture daemons; only
  the shared 1.3.0 owner remained running.

### `SMOKE-FIND-002` — thorough smoke runner encoded removed 1.2 CLI syntax

- severity: `high`
- observed: `scripts/smoke/run_thorough_shared_host.py` initially invoked
  `send --from`, `read <recipient> --as`, `ack --as`, and `list --as`, all of
  which 1.3.0 rejects. The runner therefore produced false smoke failures and
  created fixture daemons before reaching its cleanup path.
- root cause: the release smoke script was not exercised against the installed
  release CLI contract. Its command construction retained pre-1.3 flags while
  the CLI moved caller identity to `ATM_IDENTITY`.
- fix applied in this worktree: remove obsolete flags and switch recipient
  reads/acks by setting the fixture environment's `ATM_IDENTITY`.
- required regression: release preflight must execute every smoke script with
  the packaged `atm`/`atm-daemon`, assert no unsupported CLI flag is used, and
  assert the host daemon count is unchanged after the script exits.

### `SMOKE-FIND-003` — doctor unit test is not environment-isolated

- severity: `high`
- observed: `AD11-CMD-DOCTOR-001` fails because
  `execute_runs_direct_local_doctor_path` expects no daemon report, but the
  test only overrides `HOME`; inherited `ATM_DAEMON_BIN` lets
  `CliComposition::bootstrap` start a real daemon under that temporary home.
- root cause: `crates/atm/src/commands/doctor.rs` uses `EnvGuard::set_many`
  without clearing `ATM_DAEMON_BIN` or the inherited daemon bootstrap context.
- required fix: make the test set one complete isolated runtime environment
  (including an unset `ATM_DAEMON_BIN`) and add a regression that runs with a
  caller-provided daemon binary override.

### `SMOKE-FIND-004` — local compatibility hooks masked the built-in nudge

- severity: `resolved configuration finding`
- observed: the dogfood `.atm.toml` contained matching
  `[[atm.post_send_hooks]]` entries for every team member. By contract, a
  matching external hook suppresses the Rust built-in nudge, so prior nudge
  probes tested `scripts/atm-nudge.sh`, not the 1.3.0 implementation.
- resolution: removed the three compatibility-hook entries from the active
  local `.atm.toml`; the roster pane IDs were not changed.
- revalidation: with exactly one 1.3.0 daemon, raw `atm send team-lead`
  produced message `01KXD2055FH1GV0QQG0ADTMRR7`; team-lead pane `%13` received
  the exact default Rust delivery template rendered by
  `crates/atm-core/src/send/nudge_template.rs`:
  sender `arch-ctm@atm-dev`, that message ID, `read atm --team atm-dev`, the
  sent description, `execute the assigned task`, and the documented idle and
  console directives. The Bash nudge log remained at 38 lines, proving no
  external script executed.
- release implication: external post-send hooks are compatibility overrides,
  not required configuration. Release and dogfood documentation must keep them
  out of default team configuration, especially for Windows users.

### `SMOKE-FIND-005` — successful live traffic leaves daemon error records

- severity: `high`
- observed: `atm doctor` reports healthy, ready, and one owner, but the live
  daemon JSONL records `action=connection_worker`, `outcome=failed`, and
  `message="daemon local IPC connection handling failed"` for normal
  successful traffic, including the built-in nudge send at 06:17:59Z. The
  records continue after the fresh database reset.
- code root cause: `crates/atm-daemon/src/local_ipc_transport/accept_loop.rs`
  catches every `handle_connection` error and emits an error-level retained
  record, but discards the returned `AtmError` from the structured fields.
  This makes a real transport/protocol failure indistinguishable from an
  expected client disconnect or a request-domain error. `atm log snapshot`
  also fails to return these raw JSONL records because it queries normalized
  `severity` while the retained file serializes `level`.
- required fix: classify expected peer disconnects as non-errors before this
  boundary; for every remaining failure, retain the ATM error code, message,
  and request ID in the structured event. Make `atm log snapshot --level
  error` read the same schema it writes. Add an end-to-end successful
  send/read/ack test that asserts zero error-level daemon records.

### `SMOKE-FIND-006` — mandated ATM wrappers are incompatible with 1.3.0

- severity: `resolved configuration finding`
- observed: global `/Users/randlee/.local/bin/atm_ack` appends `--as
  $ATM_IDENTITY`; 1.3.0 rejects that removed flag before sending an
  acknowledgement. The wrapper's own help describes the obsolete contract.
- root cause: the nudge-era compatibility wrapper was not migrated when 1.3.0
  moved caller identity exclusively to `ATM_IDENTITY`.
- recovery: invoke `ATM_IDENTITY=<member> ATM_TEAM=<team> atm ack <id>
  <reply>` directly until the wrapper is updated. A non-ack-required message
  must receive a normal reply message instead of `atm ack`.
- required fix: update `atm_ack`, `atm_send`, and `atm_read` to the released
  CLI contract; add a versioned wrapper smoke test and prevent wrapper use
  when its advertised flags are absent from `atm --help`.
- resolution: deleted the three global wrapper links and backing scripts;
  `.atm.toml` startup guidance now directs every member to native 1.3.0
  commands. No wrapper compatibility layer remains on the dogfood host.

### `SMOKE-FIND-007` — doctor cannot disclose active post-send overrides

- severity: `observability gap`
- observed: after adding one valid `team-lead` post-send override to the active
  `.atm.toml`, `atm doctor --json` remained healthy with `config.findings: []`
  and no `post_send_hooks` field. An operator therefore cannot tell whether a
  Bash compatibility override is masking the built-in cross-platform nudge.
- code root cause: `crates/atm-core/src/doctor/mod.rs` loads `AtmConfig` but
  only uses it for team resolution and limited drift findings; it builds
  `DoctorReport.config` from `ConfigDoctorReport` and never projects
  `AtmConfig.post_send_hooks` into the report.
- required fix: add a redacted informational `post_send` doctor section showing
  built-in, external-override, and disabled-template status per recipient. For
  each external rule, show its recipient matcher, resolved executable path and
  argv, and declaring config root; for each recipient, show the selected
  delivery path. Do not expose `ATM_POST_SEND` payloads or environment values.
  Add a JSON/text regression test using one configured override. A valid
  override must not by itself downgrade doctor health or create a warning.

## AF-3 Follow-up Validation

- validation timestamp: `2026-07-14T05:56Z`
- branch: `feature/pAF-s3-native-send-input-integrity`
- binary SHA: `1cb5fb40dd6c1d4ab17ea9b0c353c6efd19b7448`
- command:
  `ATM_SMOKE_INSTALL_ROOT=<temp install> python3 scripts/smoke/run_thorough_shared_host.py`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `AF3-D3-SHARED-HOST-001` | release-binary shared-host inline/stdin/file durable readback | `PASS` | file-body expected value is now derived independently from `ATM_HOME/.config/atm/share/<team>/<filename>`; inline/stdin/file bodies all matched durable readback and invalid stdin still failed locally without changing daemon PID/count |
