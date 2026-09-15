# BENCH-V1520 official trigger failed-attempt note — 2026-09-15

## Scope

This records the v1.5.20 official benchmark invocation that did not reach
measurement. It is not a benchmark campaign and contains no performance result.


| Field | Value |
|---|---|
| Account | `atmbench@rand-m5.local` |
| Selected branch | `evidence/benchmark-v1.5.20` |
| Selected branch source | `a72ea2d62d2cc461b5adac0728e51b3751449b92` |
| Trigger | `just benchmark-official --branch evidence/benchmark-v1.5.20` over SSH |
| Measurement started | No |
| Failure boundary | Official preflight rejected a dirty checkout before source sync or matrix startup |

## Observed outcome

The dedicated benchmark account contained 23 untracked SQLite-only EQ-016
diagnostic reports from 2026-09-14/15. The official runner refused to mix
those pre-fix diagnostics with v1.5.20 campaign evidence.


```text
benchmark-official: infrastructure error: working tree is dirty before sync
```

## Cleanup proof

The 23 reports were preserved, not deleted: they were moved to the account-local
`~/benchmark-diagnostics/eq016/` archive. The checkout was then switched to the
selected evidence branch and confirmed clean. No benchmark target, temporary
