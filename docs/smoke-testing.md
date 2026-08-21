# Smoke testing

`just smoke` is the only operator entry point for ATM smoke testing. Do not
invoke a file under `scripts/smoke/` directly: those modules are implementation
details of the Just recipe and do not constitute additional test harnesses.

Use one of the following commands:

```bash
just smoke                       # normal fixture lane
just smoke fast
just smoke thorough              # fixture coverage; run graft-hermes separately for managed graft proof
just smoke localhost
just smoke local-ip
just smoke peer-preflight <host...>
just smoke crosshost-curl-plain <host...>
just smoke crosshost-send <host...>
just smoke crosshost-ack <host...>
just smoke peer-pair --config <role-config> --evidence-dir <directory>
just smoke inbound-peer --config <config> --evidence-dir <directory>
just smoke inbound-peer-combine --panes-dir <directory> --hosts <hosts> --output <file>
just smoke graft-hermes [arguments accepted by the Hermes graft smoke]
```

`just benchmark` is the separate, canonical performance gate; it is not a
replacement for functional smoke coverage. Each successful run writes its
immutable JSON, its XHTML panel, and the aggregate report beneath
`site/reports/`; the recipe rebuilds the report automatically. Use
`just benchmark-report` only to rebuild or inspect already-published evidence.

The routed smoke implementations are deliberately not independent public
commands:

| Just feature | Internal implementation | Purpose |
| --- | --- | --- |
| fixture, localhost, local-ip, peer-preflight, crosshost-* | `scripts/smoke/run_feature_smoke.py` | Standard feature ladder |
| peer-pair | `scripts/smoke/run_peer_pair.py` | Host-supplied two-role release evidence |
| inbound-peer | `scripts/smoke/run_inbound_peer_smoke.py` | Existing-daemon inbound peer evidence |
| inbound-peer-combine | `scripts/smoke/combine_inbound_peer_smoke.py` | Review existing inbound evidence panes |
| graft-hermes | `scripts/phase-ai/run_hermes_graft_live.py` | Managed-profile PyO3/Hermes graft proof with retained evidence |

All smoke artifacts remain under `reports/smoke/` or the explicitly supplied
evidence directory. The fixture-level coverage and report schema are defined
by the [smoke-test skill](../.claude/skills/smoke-test/SKILL.md).

## Staged live-daemon validation ladder

This is the required promotion sequence for any change that can affect a
canonical write, same-host adapter, peer connector, TLS, acknowledgement, or
daemon lifecycle. It prevents a two-host network symptom from obscuring a
local sender, receiver, or boundary defect.

Before the first stage, select one matched, signed CLI/daemon pair through
`daemon-switch`, then record a healthy `atm doctor --json`. Do not start,
stop, or replace a daemon manually. Each stage owns a fresh retained smoke
artifact under `site/reports/smoke/<platform>/<host>/<run-id>-pid<PID>-<feature>/`.

| Stage | Required proof | Promotion condition |
| --- | --- | --- |
| 0 | Pair readiness | Matched signed CLI/daemon pair and healthy `atm doctor --json`; record versions, binary SHA, selected wire mode, and log start offset. |
| 1 | Canonical receiver baseline | Send one valid `WriteRequest` with `curl` to the daemon's own capability-authenticated loopback endpoint. Require `201`, returned message ID, and exactly one durable local mailbox row. This is a diagnostic receiver proof only, never a substitute for an ATM transport row. |
| 2 | Host-qualified localhost | Run `just smoke localhost`. It must prove public `atm send <recipient>@<team>.localhost`, receiver read, nudge visibility, `--requires-ack`, and `atm ack`, all with exit zero. |
| 3 | Same-host advertised IP | Run `just smoke local-ip`. It must prove the same public send/read/ack semantics through the daemon's advertised same-device address. |
| 4 | Peer readiness and receiver baselines | Run `just smoke peer-preflight <host...>`, then the applicable `just smoke crosshost-curl-plain <host...>` or TLS curl baseline. Curl proves only remote listener/readiness and canonical receiver admission; it does not prove sender routing. |
| 5 | Cross-host message | Run `just smoke crosshost-send <host...>`. The sender and remote public `atm read` must report the same ULID and body. |
| 6 | Cross-host acknowledgement | Run `just smoke crosshost-ack <host...>`. It must additionally prove the returned ACK has the original acknowledged-message ID and no duplicate side effect. |

### Mandatory stop, log, and simplification gates

After every stage, before beginning the next one:

1. Retain the command result and exact artifact directory; inspect only the
   just-captured daemon-log delta with `scripts/smoke/analyze_logs.py` through
   the public `just smoke` lane. Warnings, errors, malformed/rotated logs,
   missing required events, or forbidden routing events are failures.
2. Give that artifact and log delta to an independent background reviewer.
   The reviewer must report the evidence window, every warning/error event,
   and whether the reported message ID has exactly the expected durable state.
   A stage is not promotable until this review is recorded as pass.
3. Perform an architecture and boundary review. Confirm that the candidate
   change keeps one `POST /v1/atm/messages` request/response contract, selects
   only a connector from the destination, preserves daemon-to-daemon routing,
   and adds no loopback-only route, parallel dispatcher, replay/outbox state,
   direct storage shortcut, or ambient configuration fallback. Run the
   applicable boundary and portability guards (at minimum `just lint
   boundaries same-host-portability daemon-singleton`).
4. If any check fails, stop promotion, add the finding to the active
   worktree-local checklist, reproduce it with a focused regression test, and
   make the smallest correction that closes the demonstrated defect. Repeat
   the failed stage and its independent review. Do not use a later-stage
   result to waive an earlier-stage failure.

Future sprint plans must list the applicable stages once in their authoritative
`Required Validation` section by linking to this ladder. They must not copy or
weaken the progression in a second plan-local procedure.

## Repository inventory and routing rule

This is the inventory boundary for a search of `smoke` or `smoke-test`. A
match is either an operator-facing reference that routes here, an internal
implementation, or historical/reference material. It never establishes an
alternate public runner.

| Category | Paths | Routing |
| --- | --- | --- |
| Canonical command and help | `Justfile`, `.just/print_help.py` | Run `just smoke <feature>`. |
| Operator documentation | `README.md`, `RELEASE_READINESS.md`, `docs/testing-guidelines.md`, `docs/peer-pair-smoke.md`, `docs/release-preflight-checklist.md`, `docs/team-protocol.md`, `docs/atm-graft/hermes-agent-use-case.md` | Start here, then run `just smoke <feature>`. |
| Skill documentation | `.claude/skills/smoke-test/SKILL.md` and its `references/` files | Select and run the matching `just smoke` feature. |
| Standard smoke implementation | `scripts/smoke/run_feature_smoke.py`, `run.py`, `run_thorough.py`, `run_thorough_shared_host.py`, `run_peer_pair.py`, `run_inbound_peer_smoke.py`, `combine_inbound_peer_smoke.py`, `scripts/phase-ai/run_hermes_graft_live.py` | Internal to `just smoke`; never invoke directly. The managed `graft-hermes` lane uses the already-selected Tokio/Axum pair and never creates a fixture daemon. |
| Standard smoke support and tests | `scripts/smoke/analyze_logs.py`, `daemon_lifecycle.py`, `feature_smoke_report.py`, `fixtures.py`, `phase_ad_suite.py`, `render_report.py`, `smoke_common.py`, `run_thorough_retry.py`, `test_*.py`, and `inbound-peer-smoke.example.json` | Internal code or test data; invoke the public lane through `just smoke`. |
| Specialized graft implementation | `scripts/phase-ai/run_hermes_graft_live.py`, `run-hermes-graft-smoke.py`, and their unit-test helpers | Internal to `just smoke graft-hermes`; the wrapper verifies the selected pair and registers the retained smoke report. |
| Benchmark implementation | `scripts/smoke/run_admission_capacity.py`, `benchmark_policy.py`, `benchmark_schema.py`, `benchmark_report.py`, and `.just/tests/test_benchmark_report.py` | Use `just benchmark` or `just benchmark-report`, never a direct script. |
| Other test/release helpers | `.just/run_pytests.py`, `.just/run_hermes_graft_bridge_tests.py`, `.just/tests/test_peer_pair_smoke.py`, `.just/tests/test_validate_release.py`, `scripts/test_atm_graft_python.py`, `scripts/test_atm_nudge.py`, and `scripts/validate_release.py` | Supporting test/release code; its functional smoke surface remains `just smoke`. |
| Reference-only documentation | `CHANGELOG.md`, `release/release-notes.md`, `release-findings.json`, `worktree-tracking.md`, `docs/architecture.md`, `docs/requirements.md`, `docs/project-plan.md`, `docs/cross-platform-guidelines.md`, `docs/atm-core/boundaries.md`, `docs/atm-daemon/requirements.md`, `docs/adr/ADR-007-supported-platform-parity.md`, `ADR-013-unified-delivery-plan-and-state-machine-ownership.md`, `ADR-016-claude-config-ingress-and-roster-projection-ownership.md`, `ADR-033-http-endpoint-contract.md`, `ADR-034-minimal-cross-host-https-transport.md`, `ADR-035-canonical-write-ingress-and-host-routing.md`, plus `docs/user-documents/**` examples | Descriptive only; follow this document for execution. |

Versioned plans under `docs/plans/`, generated files under `reports/`, and
retained artifacts are deliberately excluded from this routing pass: they are
historical evidence, not current operator instructions.
