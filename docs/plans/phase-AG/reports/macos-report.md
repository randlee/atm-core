# AG.1 macOS Execution Report

## Evidence Log

- execution branch/worktree:
  - branch: `feature/pAG-s1-macos-execution`
  - starting point before AG.1 execution updates: `38456732`
- binary surface under test:
  - `atm`: `/opt/homebrew/bin/atm`
  - `atm-daemon`: `/opt/homebrew/bin/atm-daemon`
  - `atm --version`: `atm 1.3.1`
- active host daemon:
  - owner lock: `/Users/randlee/.atm/daemon/owner.lock`
  - singleton owner payload: `7633:18c2571bbadd42f0`
  - live process: `PID 7633`, command `/opt/homebrew/bin/atm-daemon`

### AG-VAL-002 — macOS same-host doctor on the active 1.3.1 daemon

- command:
  - `atm doctor --json`
- result:
  - `summary.status=healthy`
  - `environment.atm_home=/Users/randlee`
  - `client_context.version=1.3.1`
  - `daemon_context.version=1.3.1`
  - `runtime_status.liveness=running`
  - `runtime_status.readiness=ready`
  - `runtime_status.singleton_owner_pid=7633`
  - `bootstrap_trace.daemon_connect=connected`
  - `bootstrap_trace.daemon_auto_start=skipped`
  - `observability.active_log_path=/Users/randlee/.atm/logs/atm.log.jsonl`
- interpretation:
  - host-singleton same-host validation is healthy on the running `1.3.1` daemon
  - this validates the macOS release-binary doctor surface against the real daemon currently owning the host runtime

### AG-VAL-002 boundary probe — temp `ATM_HOME` does not isolate the daemon/runtime

- command shape:
  - create temp `ATM_HOME`, `ATM_CONFIG_HOME`, and `ATM_LOG_DIR`
  - write minimal temp `config.json` under `/tmp/.../home/.claude/teams/atm-dev/config.json`
  - run `ATM_HOME=... ATM_CONFIG_HOME=... ATM_LOG_DIR=... ATM_DAEMON_PEER_ADDR=127.0.0.1:47001 atm doctor --json`
- result:
  - `summary.status=error`
  - `environment.atm_home=/tmp/.../home`
  - `bootstrap_trace.daemon_connect=connected`
  - `bootstrap_trace.daemon_auto_start=skipped`
  - `runtime_status.singleton_owner_pid=7633`
  - `observability.active_log_path=/Users/randlee/.atm/logs/atm.log.jsonl`
  - doctor also inspected the temp-team surface and reported:
    - missing temp inbox directory at `/tmp/.../home/.claude/teams/atm-dev/inboxes`
    - roster drift between the host ATM roster and the temp `config.json`
- linked finding:
  - `AG-FIND-003`
- interpretation:
  - `ATM_HOME` / `ATM_CONFIG_HOME` do affect the team/config compare surface
  - they do not relocate the daemon singleton, durable SQLite store, or retained log sink
  - AG.1 therefore cannot claim disposable daemon-runtime isolation on macOS without changing the supported runtime contract

### AG-VAL-011 — transport-security disposition

- requirement/plan evidence:
  - `docs/plans/phase-AG/cross-host-smoke-checklist.md` row `AG-VAL-011`
  - `docs/plans/phase-AG/cross-host-findings-ledger.md` entry `AG-FIND-001`
- implementation evidence:
  - `crates/atm-daemon/src/peer_transport.rs` reads `ATM_DAEMON_PEER_ADDR` into a socket address and uses plain TCP peer transport
  - `docs/plans/phase-AG/cross-host-findings-ledger.md` already records that the `1.3.1` workspace has no TLS-backed transport implementation for this lane
- disposition:
  - `AG-VAL-011` remains linked to open `PRODUCT-BUG` `AG-FIND-001`
  - no AG release-usable statement may claim TLS / transport-security coverage

### AG-VAL-003 viability blocker — no production inbound peer-listener lane

- code evidence:
  - `crates/atm-daemon/src/peer_transport.rs`
    - `daemon_peer_endpoint_from_env()` reads `ATM_DAEMON_PEER_ADDR`
    - `parse_peer_endpoint(...)` accepts only `SocketAddr`
    - `connect_peer_stream(...)` uses `TcpStream::connect_timeout(...)`
    - `PeerTransportRuntime` wraps a `PeerClientTransport` only
  - `crates/atm-daemon/src/composition.rs`
    - `start_background_lanes()` returns `Ok(())`
    - there is no production peer-listener startup path
  - `rg` over `crates/atm-daemon` shows `TcpListener::bind(...)` under
    `peer_transport.rs` only inside tests
- conclusion:
  - `1.3.1` ships outbound cross-host dial logic without a production inbound
    TCP peer-listener lane
  - the first live daemon-to-daemon cross-host attempt cannot succeed on the
    released build because there is nothing in production code for the remote
    daemon to bind and accept
- linked finding:
  - `AG-FIND-004`
