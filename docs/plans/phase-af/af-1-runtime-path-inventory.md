# AF-1 Runtime Path Inventory

| Call-site family | Current classification | AF-1 disposition |
| --- | --- | --- |
| `atm_core::home::{host_runtime_dir,host_runtime_lock_path,host_db_dir,host_mail_db_path}` | daemon endpoint, ownership, and durable state | canonical `HostRuntimeScope` source |
| `atm_core::protocol::daemon_socket_path` | local IPC endpoint | canonical scope; reject `ATM_DAEMON_SOCKET` |
| `atm-daemon-client` launch admission | pre-spawn lock and endpoint | canonical scope |
| `atm-daemon::host_ownership` | daemon owner lock | canonical scope |
| `atm-runtime`/daemon composition | SQLite durable state | canonical durable-state root |
| `host_runtime_*_from_home`, `daemon_socket_path_from_home` | retained test/legacy per-home derivation | delete from production admission; replace tests with cross-home rejection proof |
| `ATM_HOME` team/config/inbox lookup | workspace/config discovery | retained; never daemon admission or durable-state selection |
| `ATM_DAEMON_SOCKET` | endpoint override | rejected production override |
| direct `atm-daemon` execution | daemon launch path | same owner lock and endpoint bind |

Reconciliation command:

```bash
rg -n 'host_runtime_.*from_home|daemon_socket_path_from_home|ATM_DAEMON_SOCKET' crates scripts .github
```

Every match must be either a retained workspace/config use, a canonical scope
use, a typed rejected override, or a deletion target. No production admission
or durable-state match may remain per-`ATM_HOME`.
