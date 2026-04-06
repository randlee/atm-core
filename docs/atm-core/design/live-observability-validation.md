# Live Observability Validation

Phase K live validation was run against the real `sc-observability` adapter on
the ATM CLI binary built from this worktree. No committed
`[patch.crates-io]` entries were used; local patch overrides were supplied only
at validation time.

## Environment

- Worktree: `feature/pK-s6-integration-live-validation`
- Shared adapter source:
  `/Users/randlee/Documents/github/sc-observability/crates/sc-observability`
- Shared types source:
  `/Users/randlee/Documents/github/sc-observability/crates/sc-observability-types`
- Temporary `ATM_HOME`:
  `/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.lKji7mGmG2`

Fixture setup:

- team: `atm-dev`
- sender: `arch-ctm`
- recipient: `recipient`
- team config members:
  - `arch-ctm`
  - `recipient`

## Commands Run

The following live commands were run against the real adapter:

1. `atm send recipient@atm-dev "live snapshot seed" --json`
2. `atm read --json`
3. `atm doctor --json`
4. `atm log snapshot --match command=send --since 10m --limit 10 --json`
5. `atm log filter --match command=read --json`
6. `atm log tail --match command=send --json --poll-interval-ms 25 --max-polls 12`
7. `atm send recipient@atm-dev "live tail seed" --json` while tail was active

## Adapter State By Test

Real-adapter live runs in this pass were healthy:

- `atm doctor`: healthy
- `atm log snapshot`: healthy
- `atm log filter`: healthy
- `atm log tail`: healthy

Degraded and unavailable observability states were not induced live in this
pass because the retained shared file sink was healthy and no safe generic
failure trigger exists in the current `sc-observability` public API. Those
states remain covered by deterministic CLI integration tests:

- `crates/atm/tests/doctor.rs`
  - `test_doctor_reports_degraded_observability`
  - `test_doctor_reports_unavailable_observability_as_error`

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
      "message": "shared observability active at /var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.lKji7mGmG2/.local/share/atm/logs/atm.log.jsonl; logging health is healthy and query readiness is healthy.",
      "remediation": null
    }
  ],
  "recommendations": [],
  "environment": {
    "atm_home": "/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.lKji7mGmG2",
    "atm_team": "atm-dev",
    "atm_identity": "arch-ctm",
    "team_override": null
  },
  "observability": {
    "active_log_path": "/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.lKji7mGmG2/.local/share/atm/logs/atm.log.jsonl",
    "logging_state": "healthy",
    "query_state": "healthy",
    "detail": null
  }
}
```

Result:

- `atm doctor` projected the real adapter state correctly
- active log path matched the actual retained file sink path
- query readiness reported healthy
- the forward doctor success code is `ATM_OBSERVABILITY_HEALTH_OK`

### `atm log snapshot --match command=send --since 10m --limit 10 --json`

```json
{
  "records": [
    {
      "timestamp": "2026-04-05T04:56:28.871065Z",
      "severity": "info",
      "service": "atm",
      "target": "atm.command",
      "action": "send",
      "message": "ATM command send completed with outcome sent",
      "fields": {
        "agent": "recipient",
        "command": "send",
        "dry_run": false,
        "message_id": "2d9b78ec-8857-4686-bb8a-1f023497ee64",
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
- structured match on `command=send` worked
- emitted send records carried ATM-specific structured fields

### `atm log filter --match command=read --json`

```json
{
  "records": [
    {
      "timestamp": "2026-04-05T04:56:28.876864Z",
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

- field filtering over the shared retained store worked
- read-path lifecycle records are queryable through the same surface as send

### `atm log tail --match command=send --json --poll-interval-ms 25 --max-polls 12`

```json
{"timestamp":"2026-04-05T04:56:29.083441Z","severity":"info","service":"atm","target":"atm.command","action":"send","message":"ATM command send completed with outcome sent","fields":{"agent":"recipient","command":"send","dry_run":false,"message_id":"b48a93ce-eabf-43f7-a95c-e2064f965cb2","requires_ack":false,"sender":"arch-ctm","team":"atm-dev"}}
```

Result:

- tail mode observed the subsequent live send event through the real shared
  follow path
- the hidden `--max-polls` seam was sufficient for a bounded live validation
  run without changing the production tail contract

### Shared Log Path

Observed retained file sink:

```text
/var/folders/zk/zklzmbr52q55r1y8zv_k84k80000gn/T/tmp.lKji7mGmG2/.local/share/atm/logs/atm.log.jsonl
```

Result:

- doctor health and the filesystem agreed on the active shared log file

## Error-Code Audit

The observability error-code surface was re-audited against
`docs/atm-error-codes.md`.

Verified mappings:

| Operation | Runtime site | ATM code |
| --- | --- | --- |
| bootstrap | `ScObservabilityAdapter::new` service-name validation | `ATM_OBSERVABILITY_BOOTSTRAP_FAILED` |
| bootstrap | `ScObservabilityAdapter::new` logger init | `ATM_OBSERVABILITY_BOOTSTRAP_FAILED` |
| emit | target/action validation and `logger.emit(...)` | `ATM_OBSERVABILITY_EMIT_FAILED` |
| query | target validation, field validation, timestamp conversion, `logger.query(...)` | `ATM_OBSERVABILITY_QUERY_FAILED` |
| follow | `logger.follow(...)` start + poll failures | `ATM_OBSERVABILITY_FOLLOW_FAILED` |
| health | doctor unavailable/health-failure projection | `ATM_OBSERVABILITY_HEALTH_FAILED` |
| health success | doctor healthy projection | `ATM_OBSERVABILITY_HEALTH_OK` |
| degraded health warning | doctor degraded projection | `ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED` |

Supporting test coverage:

- `crates/atm-core/src/error.rs`
  - `observability_error_helpers_use_expected_codes`
- `crates/atm/tests/doctor.rs`
  - real-adapter healthy path
  - deterministic degraded/unavailable paths
- `crates/atm/tests/log.rs`
  - real-adapter snapshot/filter/tail paths

## Issues Found And Resolutions

Issues found during the K.6 closure work:

1. K.6 worktree did not yet include the completed K.4 fix branch or the K.5
   doctor-delivery branch.
   Resolution:
   - merged `origin/feature/pK-s4-atm-log-delivery`
   - merged `origin/feature/pK-s5-atm-doctor-delivery`
   - resolved only the expected command/output surface conflicts by preserving
     both `log` and `doctor`

2. `atm doctor` integration coverage was initially stub-only.
   Resolution:
   - added a real-adapter doctor integration test on the healthy path

3. Observability error-code claims were not backed by a direct constructor test.
   Resolution:
   - added an explicit `AtmError` observability-code mapping test

No additional runtime issues were found in the healthy real-adapter live pass.
