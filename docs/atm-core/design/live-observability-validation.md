# Live Observability Validation

Phase L.2 re-ran live validation against the published
`sc-observability = "1.0.0"` adapter on the ATM CLI binary built from this
worktree. This pass closes the earlier healthy-only gap by exercising healthy,
degraded, and unavailable observability states through the shared retained-sink
fault injector rather than through ATM-local deterministic doubles.

## Environment

- Worktree: `feature/pL-s2-fault-injection`
- ATM binary:
  `/Users/randlee/Documents/github/atm-core-worktrees/feature/pL-s2-fault-injection/target/debug/atm`
- Published dependencies:
  - `sc-observability = "1.0.0"`
  - `sc-observability-types = "1.0.0"`
- Healthy/degraded/unavailable fixture `ATM_HOME`:
  `/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq`
- Tail fixture `ATM_HOME`:
  `/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.ELpCcLpmlE`

Fixture setup:

- team: `atm-dev`
- sender: `arch-ctm`
- recipient: `recipient`
- team config members:
  - `arch-ctm`
  - `recipient`

## Commands Run

Healthy retained-adapter run:

1. `atm send recipient@atm-dev "live snapshot seed" --json`
2. `atm read --json`
3. `atm doctor --json`
4. `atm log snapshot --match command=send --since 10m --limit 10 --json`
5. `atm log filter --match command=read --json`

Live fault-injected doctor runs through the shared adapter:

6. `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=degraded atm doctor --json`
7. `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=unavailable atm doctor --json`

Live follow/tail run:

8. `atm log tail --match command=send --json --poll-interval-ms 25`
9. `atm send recipient@atm-dev "tail capture" --json` while tail was active

## Validation Harness Notes

Phase L.2 adds one validation-only environment seam owned by ATM:

- `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=degraded|unavailable`

Behavior:

- ATM still uses the real shared `Logger` and the real `RetainedSinkFaultInjector`
  from `sc-observability`
- the seam adds one extra retained sink wrapped by the shared injector
- `atm doctor` therefore observes degraded and unavailable logging states
  through the ordinary shared `Logger::health()` path
- deterministic ATM integration tests remain the fast/stable regression layer;
  this live seam supplements them with real shared-adapter validation

## Adapter State By Test

Observed real-adapter states in this pass:

- `atm doctor`: healthy
- `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=degraded atm doctor`: degraded
- `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=unavailable atm doctor`: unavailable
- `atm log snapshot`: healthy
- `atm log filter`: healthy
- `atm log tail`: healthy

The degraded/unavailable runs now come from the shared crate directly instead
of from ATM-local doubles.

## Captured Output

### `atm doctor --json`

```json
{
  "summary": {
    "status": "healthy",
    "message": "ATM doctor completed with healthy findings only",
    "info_count": 1,
    "warning_count": 0,
    "error_count": 0
  },
  "findings": [
    {
      "severity": "info",
      "code": "ATM_OBSERVABILITY_HEALTH_OK",
      "message": "shared observability active at /var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq/.local/share/logs/atm.log.jsonl; logging health is healthy and query readiness is healthy.",
      "remediation": null
    }
  ],
  "recommendations": [],
  "environment": {
    "atm_home": "/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq",
    "atm_team": "atm-dev",
    "atm_identity": "arch-ctm",
    "team_override": null
  },
  "observability": {
    "active_log_path": "/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq/.local/share/logs/atm.log.jsonl",
    "logging_state": "healthy",
    "query_state": "healthy",
    "detail": null
  }
}
```

Result:

- `atm doctor` projected the healthy shared adapter state correctly
- active log path matched the current shared file-sink layout
- query readiness reported healthy

### `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=degraded atm doctor --json`

```json
{
  "summary": {
    "status": "warning",
    "message": "ATM doctor completed with warnings",
    "info_count": 0,
    "warning_count": 1,
    "error_count": 0
  },
  "findings": [
    {
      "severity": "warning",
      "code": "ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED",
      "message": "shared observability is degraded at /var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq/.local/share/logs/atm.log.jsonl; logging health is degraded and query readiness is healthy.",
      "remediation": "Inspect the shared log store and query path, then re-run `atm doctor`."
    }
  ],
  "recommendations": [
    "Inspect the shared log store and query path, then re-run `atm doctor`."
  ],
  "environment": {
    "atm_home": "/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq",
    "atm_team": "atm-dev",
    "atm_identity": "arch-ctm",
    "team_override": null
  },
  "observability": {
    "active_log_path": "/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq/.local/share/logs/atm.log.jsonl",
    "logging_state": "degraded",
    "query_state": "healthy",
    "detail": null
  }
}
```

Result:

- the shared retained-sink fault injector drove a warning-grade degraded state
  through the real adapter
- ATM preserved the healthy query surface while correctly reporting degraded
  logging health

### `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=unavailable atm doctor --json`

```json
{
  "summary": {
    "status": "error",
    "message": "ATM doctor found critical issues",
    "info_count": 0,
    "warning_count": 0,
    "error_count": 1
  },
  "findings": [
    {
      "severity": "error",
      "code": "ATM_OBSERVABILITY_HEALTH_FAILED",
      "message": "shared observability is unavailable; active log path is /var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq/.local/share/logs/atm.log.jsonl and query readiness is healthy.",
      "remediation": "Restore shared observability initialization and confirm the active log path is writable."
    }
  ],
  "recommendations": [
    "Restore shared observability initialization and confirm the active log path is writable."
  ],
  "environment": {
    "atm_home": "/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq",
    "atm_team": "atm-dev",
    "atm_identity": "arch-ctm",
    "team_override": null
  },
  "observability": {
    "active_log_path": "/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq/.local/share/logs/atm.log.jsonl",
    "logging_state": "unavailable",
    "query_state": "healthy",
    "detail": null
  }
}
```

Result:

- the shared retained-sink fault injector drove an error-grade unavailable
  state through the real adapter
- `atm doctor` exited non-zero while still emitting the full JSON report

### `atm log snapshot --match command=send --since 10m --limit 10 --json`

```json
{
  "records": [
    {
      "timestamp": "2026-04-07T04:38:26.040228Z",
      "severity": "info",
      "service": "atm",
      "target": "atm.command",
      "action": "send",
      "message": "ATM command send completed with outcome sent",
      "fields": {
        "agent": "recipient",
        "command": "send",
        "dry_run": false,
        "message_id": "0c45aa64-3974-4fa3-a375-613c4fa8a361",
        "requires_ack": false,
        "sender": "arch-ctm",
        "team": "atm-dev"
      }
    }
  ],
  "truncated": false
}
```

Result:

- snapshot mode read from the real shared file-backed store
- send-path emission remains queryable through the shared retained log

### `atm log filter --match command=read --json`

```json
{
  "records": [
    {
      "timestamp": "2026-04-07T04:38:26.044913Z",
      "severity": "info",
      "service": "atm",
      "target": "atm.command",
      "action": "read",
      "message": "ATM command read completed with outcome ok",
      "fields": {
        "agent": "arch-ctm",
        "command": "read",
        "dry_run": false,
        "requires_ack": false,
        "sender": "arch-ctm",
        "team": "atm-dev"
      }
    }
  ],
  "truncated": false
}
```

Result:

- read-path lifecycle records remain queryable through the shared retained log

### `atm log tail --match command=send --json --poll-interval-ms 25`

```json
{"timestamp":"2026-04-07T04:38:42.280917Z","severity":"info","service":"atm","target":"atm.command","action":"send","message":"ATM command send completed with outcome sent","fields":{"agent":"recipient","command":"send","dry_run":false,"message_id":"6589d24b-656a-453a-8c63-ea22cbd223d2","requires_ack":false,"sender":"arch-ctm","team":"atm-dev"}}
```

Result:

- tail mode observed the subsequent live send event through the real shared
  follow path without relying on the hidden test-only `--max-polls` seam

## Carry-In Closure

Phase L.2 closes both Phase K observability carry-in items:

- `ATM-QA-K-001`
  - retained-log emission is now integration-tested for:
    - `send`
    - `read`
    - `ack`
    - `clear`
- `ATM-QA-K-002`
  - degraded and unavailable doctor paths are now exercised through the real
    shared adapter using the published fault-injection API

Supporting test coverage:

- `crates/atm/tests/doctor.rs`
  - healthy real-adapter path
  - degraded real-adapter fault-injection path
  - unavailable real-adapter fault-injection path
- `crates/atm/tests/send.rs`
  - retained-log emission for `send`
- `crates/atm/tests/read.rs`
  - retained-log emission for `read`
- `crates/atm/tests/ack.rs`
  - retained-log emission for `ack`
- `crates/atm/tests/clear.rs`
  - retained-log emission for `clear`
- `crates/atm/tests/log.rs`
  - shared snapshot/filter/tail behavior

## Production Diagnosis

Current dependency assumptions:

- ATM consumes `sc-observability = "1.0.0"` and
  `sc-observability-types = "1.0.0"` from crates.io
- retained observability uses the shared JSONL file sink by default

Standard log-file path:

- inspect `atm doctor --json` first and use `observability.active_log_path` as
  the source of truth
- on a standard install, the retained file sink is expected under:
  `$ATM_HOME/.local/share/logs/atm.log.jsonl`

How to interpret `atm doctor`:

- `healthy`
  - shared observability initialized successfully
  - retained log writes are healthy
  - retained query readiness is healthy
- `degraded`
  - retained logging is dropping or partially unhealthy
  - query readiness may still be healthy
- `unavailable`
  - retained logging is unavailable
  - `atm doctor` exits non-zero and operators should restore the shared
    observability sink before trusting retained diagnostics
