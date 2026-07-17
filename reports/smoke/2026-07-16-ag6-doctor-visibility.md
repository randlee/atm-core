# AG6 Doctor Visibility Smoke Artifact

- timestamp: `2026-07-16T03:04:57Z`
- branch: `feature/pAG-s6-doctor-visibility`
- purpose: prove the AG.6 `cross_host` doctor projection renders durable
  interface rows, bound endpoint state, and allowlist rows from SQLite-backed
  daemon state

## Commands

```bash
cargo fmt --all
ATM_EMIT_DOCTOR_FIXTURE=1 cargo test -p atm-daemon tests::doctor_projects_cross_host_interface_and_allowlist_state_from_sqlite -- --exact --nocapture
cargo test -p atm-daemon tests::doctor_warns_when_cross_host_listener_is_unconfigured_and_allowlist_is_empty -- --exact
cargo test -p atm-daemon tests::doctor_surfaces_degraded_cross_host_bind_state_and_staleness -- --exact
```

## Recorded Output

- raw doctor report JSON: `reports/smoke/2026-07-16-ag6-doctor-visibility.json`
- positive fixture: one enabled durable interface row (`vpn0`), one enabled
  allowlist host (`10.10.100.98`), one disabled allowlist host
  (`10.10.100.99`)

Key observed doctor fields:

- `cross_host.legacy_fallback_active = false`
- `cross_host.bound_endpoints = ["10.10.100.10:43101"]`
- `cross_host.interfaces[0].interface_name = "vpn0"`
- `cross_host.interfaces[0].listener_bound = true`
- `cross_host.allowlist.enforced = true`
- `cross_host.allowlist.empty = false`
- `cross_host.allowlist.hosts` contains:
  - enabled row `10.10.100.98`
  - disabled row `10.10.100.99`

## Verdict

`PASS` — AG.6 doctor visibility is backed by a concrete recorded doctor report,
and the two companion tests cover the empty-allowlist warning path and the
degraded/stale bind projection path.
