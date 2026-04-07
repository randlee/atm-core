# Live Observability Validation

Phase L.2 re-ran live validation against the published
`sc-observability = "1.0.0"` adapter on the ATM CLI binary built from this
worktree. This pass closes the earlier healthy-only gap by exercising healthy,
degraded, and unavailable observability states through the shared retained-sink
fault injector rather than through ATM-local deterministic doubles.

## Phase L.6 Rerun (2026-04-07)

Phase L.6 reran the published-baseline validation after the L.4 public field
cleanup, the L.5 construction refactor, and the release-closeout identity
changes. The goal of this rerun was to prove that the shared observability
surface still behaves the same on the published `1.0.0` crates while also
confirming the new doctor drift finding for obsolete `[atm].identity`.

Rerun fixtures:

- clean validation fixture `ATM_HOME`:
  `/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.MGlw9wjiob`
- drift-check fixture `ATM_HOME`:
  `/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.TVHup79rTM`

Commands rerun on the clean fixture:

1. `atm send arch-ctm@atm-dev "l6 validation seed" --json`
2. `atm read --json`
3. `atm doctor --json`
4. `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=degraded atm doctor --json`
5. `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=unavailable atm doctor --json`
6. `atm log snapshot --match command=send --since 10m --limit 10 --json`
7. `atm log filter --match command=read --json`

Additional drift-check command:

8. `atm doctor --json` with `.atm.toml` containing obsolete `[atm].identity`

Observed results:

- `atm send` succeeded and `atm read` returned one unread message from the
  clean fixture inbox
- healthy doctor run stayed `healthy` with `ATM_OBSERVABILITY_HEALTH_OK`
- degraded fault-injected doctor run stayed `warning` with
  `ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED`
- unavailable fault-injected doctor run stayed `error` with
  `ATM_OBSERVABILITY_HEALTH_FAILED` and exited with status `1`
- `atm log snapshot` still returned the emitted `send` command record
- `atm log filter` still returned the emitted `read` command record
- the active shared file sink remained:
  `/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.MGlw9wjiob/.local/share/logs/atm.log.jsonl`
- the separate drift-check run returned warning status with:
  - `ATM_WARNING_IDENTITY_DRIFT`
  - `ATM_OBSERVABILITY_HEALTH_OK`

Interpretation:

- L.4 and L.5 did not change the published shared-adapter wire behavior for
  ATM observability commands
- L.6 closes the identity carry-forward findings without disturbing the
  healthy/degraded/unavailable health mapping already validated in L.2
- obsolete `[atm].identity` now surfaces as an additive doctor warning rather
  than a runtime identity fallback

## Environment

- Worktree: `feature/pL-s2-fault-injection`
- ATM binary: `./target/debug/atm` from the checked-out worktree root
- Published dependencies:
  - `sc-observability = "1.0.0"`
  - `sc-observability-types = "1.0.0"`
- Healthy/degraded/unavailable fixture `ATM_HOME`:
  `/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.IazCSMniAq`
- Tail fixture `ATM_HOME`:
  `/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.ELpCcLpmlE`

Machine-specific note:

- the temporary directory prefixes shown in captured output below come from one
  local validation host; only the retained-log suffix is normative:
  `$ATM_HOME/.local/share/logs/atm.log.jsonl`

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

## Phase L.3 Follow-Up Validation

Phase L.3 re-ran the live doctor/snapshot path specifically to prove that the
post-`#21` file-sink layout is the operator-facing path ATM should document.

Worktree:

- `feature/pL-s3-file-sink-migration`

Commands run:

1. `atm send recipient@atm-dev "l3 path seed" --json`
2. `atm doctor --json`
3. `atm log snapshot --match command=send --since 10m --limit 10 --json`

Observed results:

- `atm doctor --json` reported
  `observability.active_log_path ==
  <ATM_HOME>/.local/share/logs/atm.log.jsonl`
- `atm log snapshot` returned the retained `send` record emitted into that same
  migrated store
- no live command in this pass depended on the older
  `<log_root>/<service>.log.jsonl` layout

Captured excerpts:

```json
{
  "observability": {
    "active_log_path": "/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.WybcP84GtN/.local/share/logs/atm.log.jsonl",
    "logging_state": "healthy",
    "query_state": "healthy"
  }
}
```

```json
{
  "records": [
    {
      "action": "send",
      "fields": {
        "command": "send",
        "message_id": "1987711b-fe98-4e35-bb6c-cf65b4424ade"
      }
    }
  ],
  "truncated": false
}
```

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
