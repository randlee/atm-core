# AG.1 Windows Execution Report

## Evidence Log

### 2026-07-15T05:30Z — Windows AG.1 setup dry-run

- branch: `feature/pAG-s1-macos-execution`
- worktree: `F:\github\atm-core-worktrees\feature\pAG-s1-macos-execution`
- local branch state: `8f2d5f53` after merging `origin/develop` into the sprint branch because the checked-out remote branch lacked `plan-phase-AG.md` and `sprint-AG1.md`
- release binary build: `cargo build --release -p agent-team-mail -p atm-daemon` exited `0`
- `atm --version`: `atm 1.3.1`
- resolved `atm`: `F:\github\atm-core-worktrees\feature\pAG-s1-macos-execution\target\release\atm.exe`
- resolved `atm-daemon`: `F:\github\atm-core-worktrees\feature\pAG-s1-macos-execution\target\release\atm-daemon.exe`
- placeholder peer for same-host-only dry-run: `ATM_DAEMON_PEER_ADDR=127.0.0.1:9`
- actual macOS peer address: not available, so no first-live-channel attempt was made

Attempt 1 used only the runbook-listed disposable directories:

- `ATM_HOME=<artifact>\atm-home`
- `ATM_CONFIG_HOME=<artifact>\atm-config-home`
- `ATM_LOG_DIR=<artifact>\atm-log-dir`
- `ATM_TEAM=ag-clean-room`
- `ATM_IDENTITY` was initially omitted

Result:

- `atm doctor --json`: exit `1`
- `atm list --json`: exit `3`
- `atm clear --json`: exit `3`
- `atm read --all --json`: exit `3`
- failure cause: missing `.claude/teams/ag-clean-room/config.json` and missing `ATM_IDENTITY`

Attempt 2 added `ATM_IDENTITY=windows-operator` and tried to seed the team
with `atm teams add-member`.

Result:

- `atm teams add-member ag-clean-room windows-operator ... --json`: exit `3`
- `atm teams add-member ag-clean-room windows-peer ... --json`: exit `3`
- failure cause: `team 'ag-clean-room' was not found`; the runbook was missing the initial `config.json` bootstrap requirement

Attempt 3 seeded disposable `ATM_HOME\.claude\teams\ag-clean-room\config.json`
with `{"members":[]}` and redirected `HOME` to a disposable host-home path.

Command results:

- `atm teams add-member ag-clean-room windows-operator --home-dir <worktree> --agent-type codex-cli --model windows --json`: exit `0`
- `atm teams add-member ag-clean-room windows-peer --home-dir <worktree> --agent-type general-purpose --model windows --json`: exit `0`
- `atm teams --json`: exit `0`
- `atm members --team ag-clean-room --json`: exit `0`
- `atm doctor --json`: exit `0`; summary `healthy`, `warning_count=0`, `error_count=0`, runtime `liveness=running`, `readiness=ready`, daemon PID `26116`
- `atm list --json`: exit `0`
- `atm clear --json`: exit `0`
- `atm send windows-peer "AG.1 Windows clean-room same-host probe" --json`: exit `0`, message ID `01KXJ43JDHB9ED6WDDGDD63VWB`
- `ATM_IDENTITY=windows-peer atm read --all --json`: exit `0`, read message ID `01KXJ43JDHB9ED6WDDGDD63VWB`

However, Attempt 3 is not accepted as clean-room `AG-VAL-001` evidence:

- `crates/atm-core/src/home.rs` shows `current_host_runtime_scope()` uses `os_account_home()?.join(".atm")`
- that function explicitly ignores `ATM_HOME`, `HOME`, `USERPROFILE`, and the current directory
- the Windows run wrote two `team_roster` rows and one probe message/state row into `C:\Users\rand.lee\.atm\db\mail.db`
- I deleted exactly those rows after discovery:
  - `mail_message_states`: 1 row for `ag-clean-room` / `atm:01KXJ43JDHB9ED6WDDGDD63VWB`
  - `mail_messages`: 1 row for `ag-clean-room` / `atm:01KXJ43JDHB9ED6WDDGDD63VWB`
  - `team_roster`: 2 rows for `ag-clean-room`
  - remaining rows for `ag-clean-room`: `0`

Classification:

- `AG-VAL-001`: `BLOCKED`, linked to `AG-FIND-002`
- `AG-VAL-011`: `FAIL`, linked to existing `AG-FIND-001`
- first live Windows -> macOS channel viability: not attempted because the macOS peer address was not available and `AG-VAL-001` clean-room isolation is blocked

Transport-security evidence for `AG-VAL-011`:

- `crates/atm-daemon/src/peer_transport.rs` imports and uses `std::net::TcpStream`
- `rg` found no `native-tls`, `rustls`, `tokio-rustls`, or `openssl` dependency in `Cargo.toml` / `Cargo.lock`
- existing `AG-FIND-001` remains the correct linked finding
