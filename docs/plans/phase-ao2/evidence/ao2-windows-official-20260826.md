# AO2 Windows Official Benchmark Report

- Branch: `bench/ao2-windows-official`
- Host: `windows-x64-01` (`windows`, `amd64`)
- Final source/fix commit: `c387a45d1`
- Final campaign: `20260826T164714Z-windows-x64-01`
- Required matrix: `sqlite`, `tcp`, `tcp-tls` at f8
- Account: dedicated benchmark account with a fresh `C:\\Users\\rand.lee\\.atm` manifest

## Changes and justification

1. `Justfile` now invokes `.just/run_benchmark.py` instead of using POSIX-only
   `|| benchmark_status=$?`, `${benchmark_status:-0}`, and `exit` syntax. The
   old recipe raised a PowerShell parse error when the runner returned its
   expected measured-below-floor status, preventing report rebuild/publish on
   Windows. The Python wrapper preserves the runner verdict and always rebuilds
   the reports and index. This is shell-portable and does not change benchmark
   semantics.
2. `scripts/smoke/run_admission_capacity.py` checkpoints the owned SQLite
   database after the daemon is reaped and before snapshot validation.
   Windows process termination can leave SQLite `-wal`/`-shm` sidecars after
   `taskkill`; the existing safety gate correctly rejected those sidecars as an
   unsafe restore candidate. The checkpoint removes only sidecars for the
   stopped, manifest-owned benchmark database. A regression test covers WAL
   cleanup. This uses SQLite's cross-platform API and does not change product
   transport behavior.
3. Added a provisional `windows-x64-01` SQLite baseline entry so the required
   Windows three-target matrix can execute. It is explicitly marked pending
   quality review and must not be treated as an approved floor. No existing
   macOS or Linux floor was changed.

## Validation

Focused validation on Windows:

- Python compilation: passed.
- `scripts/smoke/test_benchmark_snapshot.py`: `16/16 OK`.
- `just benchmark`: completed all three targets and published a measured
  campaign; exit `1` is the expected below-floor verdict.
- `just benchmark-publish`: passed.
- `just benchmark-show`: report rebuild passed, but the final Wyvern open step
  returned `[WinError 2]` because Wyvern is not installed on this host. The
  machine-readable and HTML report artifacts were still generated and
  published.

## Final campaign metrics

| Target | Requested | Admitted | Durable after restart | p50 msg/s | Pending floor | Status |
| --- | ---: | ---: | --- | ---: | ---: | --- |
| SQLite | 318,000 | 318,000 | yes | 16,022.88 | 16,035.35 | FAIL |
| TCP | 118,000 | 118,000 | yes | 6,012.47 | 8,793.91 | FAIL |
| TCP + TLS | 114,000 | 114,000 | yes | 5,614.01 | 6,891.68 | FAIL |

All three targets completed their intervals with no first failure and exact
post-restart durability counts. The FAIL statuses are performance-floor
comparisons only; this report does not claim a benchmark pass.

## Retained artifacts

Final campaign JSON:

- `site/reports/send-message-benchmark/20260826T164714Z-windows-x64-01.campaign.json`
- `site/reports/send-message-benchmark/20260826T164714Z-windows-x64-01-sqlite.json`
- `site/reports/send-message-benchmark/20260826T164714Z-windows-x64-01-tcp.json`
- `site/reports/send-message-benchmark/20260826T164714Z-windows-x64-01-tcp-tls.json`

Raw local traces:

- `artifacts/benchmark/send-message-benchmark/20260826-164714.330767-windows-x64-01-sqlite-f8.json`
- `artifacts/benchmark/send-message-benchmark/20260826-164736.438338-windows-x64-01-tcp-f8.json`
- `artifacts/benchmark/send-message-benchmark/20260826-164759.202598-windows-x64-01-tcp-f8.json`

The branch also retains the first failed SQLite sidecar diagnostic and its
successful post-fix diagnostic as immutable evidence.
