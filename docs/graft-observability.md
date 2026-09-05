# Graft fallback observability

The `atm-graft-python` binding records recovery diagnostics in the dedicated
`atm-graft-fallback.jsonl` satellite. It never opens, rotates, or appends to
the daemon's canonical `atm.log.jsonl` stream.

## Paths

`atm_graft.observability_paths()` resolves the same log directory inputs as
the host runtime and returns Rust-owned `log_dir`, `canonical_log_path`, and
`fallback_log_path` values. Resolution precedence is:

1. `ATM_LOG_DIR` (`log_dir_source = "env:ATM_LOG_DIR"`)
2. `ATM_HOME/.atm/logs` (`log_dir_source = "env:ATM_HOME"`)
3. the platform default (`log_dir_source = "default"`)

Hermes does not construct or normalize these paths.

## Recovery events

When a graft `atm_list`, `atm_read`, `atm_send`, or `atm_ack` call first sees
the canonical daemon-unavailable error, the binding emits allowlisted JSONL
events with `origin = "graft"`:

| Code | Additional fields |
| --- | --- |
| `ATM_GRAFT_DAEMON_UNAVAILABLE` | `endpoint_kind`, `failure_class`, `strategy`, `correlation_id`; endpoint failure also has `refresh_error_code` |
| `ATM_GRAFT_RECOVERY_ATTEMPT` | `attempt`, `strategy` |
| `ATM_GRAFT_RECOVERY_RESULT` | `outcome` (`recovered` or `failed`), `elapsed_ms` |
| `ATM_GRAFT_FALLBACK_WRITE_FAILED` | `error_layer` when the satellite cannot retain a diagnostic |

`endpoint_kind` is `unix_domain_socket` or `tcp_loopback`; `failure_class` is
`stale_client` or `endpoint_unavailable`. Recovery never replays a send.

## Bounded failure behavior

The satellite writer uses a bounded queue of 256 entries and rotates its
active file at 2 MiB, retaining at most three satellite files. Events contain
only the shared `RETAINED_FIELD_ALLOWLIST`; message bodies, recipients,
chat identifiers, tokens, and environment values are not serialized.

If a fallback write fails, the primary operation keeps its original result.
The typed result or error may carry:

```json
{"observability":{"fallback_write_failed":true,"code":"ATM_GRAFT_FALLBACK_WRITE_FAILED"}}
```

This diagnostic is best-effort and is never allowed to turn a successful
message send or acknowledgement into a failed operation.
