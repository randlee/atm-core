# cwin task: RBQA-F001

Branch: feature/pAI-s16-offline-reconciliation (this branch)

crates/atm-daemon/src/local_ipc_transport.rs — `bind_after_install` (~line 293-299)
binds both a `#[cfg(unix)] tcp_loopback: LocalTcpLoopbackServer` field AND the UDS
`LocalSocketListener`; both are served concurrently in `serve_runtime_scope`
(lines 458, 752).

Client (`atm-daemon-client/src/lib.rs:148-151`, `resolve_daemon_local_ipc_endpoint`)
resolves exclusively via `local_http_record_path` — never touches the TCP loopback.

Fix: remove the dead TCP-loopback bind/serve path; keep UDS only.

Report: branch + SHA, `just lint && just test` result.
