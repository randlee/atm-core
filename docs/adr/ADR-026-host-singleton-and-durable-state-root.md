# ADR-026 — Host Singleton And Durable State Root

| Field | Value |
| --- | --- |
| ID | ADR-026 |
| Status | **Accepted** |
| Supersedes | ADR-002, ADR-005 |
| Relates to | REQ-P-RUNTIME-002, REQ-P-RUNTIME-003, REQ-P-RUNTIME-004 |

## Decision

For one OS user on one host, ATM has exactly one non-configurable
`HostRuntimeScope`. It owns the daemon endpoint, `launch.lock`, `owner.lock`,
and the one durable SQLite root. The scope is derived from the OS-user runtime
location, never from `ATM_HOME`, `ATM_DAEMON_SOCKET`, the working directory,
or a test-only override.

`ATM_HOME` remains workspace/config discovery only. A supplied
`ATM_DAEMON_SOCKET` is rejected before bootstrap; it cannot select a second
endpoint. The client launch lock, daemon owner lock, and endpoint bind are
independent barriers and all use the same scope.

Existing databases are recovered into the canonical durable-state root through
the documented one-time recovery procedure, before any new daemon serves the
team. Recovery is ownership-gated and fails closed on unsafe permissions,
symlink traversal, or ambiguity; it never creates a parallel serving database.

## Platform Contract

- Unix platforms use the current OS-user home/runtime location and a
  same-user local IPC endpoint.
- Windows uses the corresponding current-user runtime location and the
  loopback-TCP endpoint record/capability defined by ADR-033.
- The common path API returns semantic `HostRuntimeRoot` and
  `DurableStateRoot` wrappers with `AsRef<Path>` only; neither exposes `Deref`.

## Legacy Database Recovery

Before first 1.3.1 startup, stop every ATM daemon and preserve the former
`$ATM_HOME/.atm/db/mail.db`. If the canonical database does not yet exist,
copy that file to the OS-account `~/.atm/db/mail.db` (or the equivalent
Windows profile path), then start exactly one daemon. If both files exist,
do not choose a winner automatically: retain both backups and perform an
explicit SQLite merge/verification before serving. A workspace must never
silently create a new database merely because it has a different `ATM_HOME`.

## Consequences

- distinct workspaces and `ATM_HOME` values share one daemon and one database
- direct daemon invocation is subject to the same owner gate as CLI auto-start
- tests run under an isolated OS user/CI host and may not introduce alternate
  runtime roots, endpoints, or locks
- ADR-002 and ADR-005 remain historical accepted records; this ADR supersedes
  their runtime-root details without deleting their rationale
