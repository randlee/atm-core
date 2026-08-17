# ATM Daemon Client Requirements

`atm-daemon-client` owns shared same-host daemon bootstrap and transport-envelope
helpers for thin clients. It does not own daemon dispatch, runtime composition,
SQLite, or business workflow semantics.

## AF requirements

- The client must use the single OS-user host-runtime admission scope selected
  by planned ADR-026; `ATM_HOME`, current directory, and `ATM_DAEMON_SOCKET` cannot
  create a second launch gate or endpoint.
- A contended client admission connects to the serving singleton or returns a
  typed, recoverable admission error; it must not spawn a second daemon.
- Bootstrap and RPC failures preserve `AtmErrorCode`, cause context, and
  recovery through the CLI render boundary.
- Before a write-shaped RPC, the client sends the ADR-027 compatibility
  preflight after local-IPC connection. An incompatible daemon returns
  a typed schema/HTTP-major compatibility error with recovery guidance and
  receives no write-shaped request. Product release versions are diagnostic;
  a compatible release mismatch must not block dispatch.
- RPC message-source DTOs must not represent caller stdin. The CLI consumes
  stdin before RPC and transmits a materialized body.
- This crate must retain the dependency and extension restrictions in
  `boundaries/atm-daemon-client/{daemon-bootstrap,rpc-envelope}.toml`.
