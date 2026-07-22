# cwin task: RBQA-F001

Branch: feature/pAI-s16-offline-reconciliation (this branch)

CORRECTION (previous version of this file had the fix direction backwards — do not
apply as originally written):

crates/atm-daemon/src/local_ipc_transport.rs — `bind_after_install` (~line 293-299)
binds both a `#[cfg(unix)] tcp_loopback: LocalTcpLoopbackServer` field AND the UDS
`LocalSocketListener`; both are served concurrently in `serve_runtime_scope`
(lines 458, 752).

Client (`atm-daemon-client/src/lib.rs:148-151`, `resolve_daemon_local_ipc_endpoint`,
non-Windows branch) resolves via `local_http_record_path` — i.e. it uses the TCP
loopback, not UDS. This was a deliberate AI.14 migration (dbd112e2 "use local HTTP
endpoint on Unix", c5665204 "remove retired local socket client dependency") — not
an accident.

So UDS (`LocalSocketListener`) is the dead code, not the TCP loopback.

Fix: remove the dead UDS bind/serve path; keep the TCP loopback (local_http_record_path)
as the sole Unix local-IPC transport.

Report: branch + SHA, `just lint && just test` result.
