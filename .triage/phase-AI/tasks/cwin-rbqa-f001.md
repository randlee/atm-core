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

---

RETRACTED — see AI11-QA5-RBQA-F001.ttl / AI14-QA1-RBQA-F001.ttl (status: scope-mismatch).

Do NOT remove the TCP-loopback path. REQ-CORE-TRANSPORT-001 (docs/requirements.md
sec 22.4) requires: "Unix same-host clients use HTTP over UDS and may use HTTP
over loopback TCP; Windows same-host clients use HTTP over loopback TCP only."
Windows has no UDS/named-pipe path at all -- it is TCP-only. Unix/Mac must keep
BOTH UDS and TCP-loopback bound, for Unix/Windows parity testing per the same
requirement ("Unix/Windows parity requires equivalent local HTTP
request/response tests: UDS plus loopback TCP on Unix and loopback TCP on
Windows"). The dual bind in bind_after_install is correct, required behavior,
not dead code.

No action needed on this task. If a real client-side gap exists (e.g.
atm-daemon-client only resolving via local_http_record_path and never
exercising the UDS path), that is a separate, not-yet-triaged question -- do
not infer it from this file.
