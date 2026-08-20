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
Use its public peer-wire targets, not a Cargo feature or a private harness:

```bash
just benchmark --target tcp       # direct TCP, explicit plaintext-test daemon
just benchmark --target tcp-tls   # same daemon, ordinary mutual-TLS launch mode
```

Both targets build the same shipped daemon and differ only by the documented
`--peer-wire-security` launch argument. Every result records candidate commit,
binary version, selected mode, host OS/architecture, hook mode,
frames-per-connection, sample count, sanitized command, and compatible
baseline identity. `tcp` may compare only with a same-host plaintext baseline;
the first accepted `tcp-tls` campaign establishes its own same-mode baseline.
Peer-wire evidence must record the active ADR-047 security mode. Normal
benchmark and release evidence is mutual TLS; `plaintext-test` is explicit
diagnostic evidence only and cannot satisfy mTLS or peer-allowlist criteria.

The daemon selects this mode only at process launch. Verify the selected value
with `atm doctor --json` before a peer-wire smoke. A disposable
`--peer-wire-security plaintext-test` daemon uses the preserved direct TCP
pipeline for connectivity diagnostics; a normal restart without that argument
returns to the mTLS default. A failed mTLS configuration or handshake is a
failure to repair, never a reason to retry the same run in plaintext.

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
