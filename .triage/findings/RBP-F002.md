# RBP-F002: Silent lock-poison swallow

## Pattern
```
\.lock\(\)\.unwrap_or_else
PoisonError
unwrap_or_else.*into_inner
\.lock\(\)\.unwrap\(\)
mark_sqlite_unavailable
ActiveConnectionRegistry::remove
```

## Crates Affected
- atm-daemon

## Sprint Origin
R.14 (first reported R.14-QA-5 as RBP-F002)
R.15 (reported R.15-QA-3 as RBP-F001/F002 in runtime_health.rs)

## Status
open

## Description
Lock-poison errors are silently swallowed via `.unwrap_or_else(|e| e.into_inner())` or equivalent, discarding the poisoning signal. A poisoned lock means another thread panicked while holding it — silently continuing with poisoned state propagates corruption. Either: (a) propagate the error, (b) log + propagate, or (c) document explicitly why poison-recovery is safe here.

Key sites:
- `mark_sqlite_unavailable` in runtime_health.rs — silently swallows lock-poison
- `ActiveConnectionRegistry::remove` in R.14

## Occurrences
| Branch | File | Line | Snippet | Fixed |
|--------|------|------|---------|-------|
| R.14 | crates/atm-daemon/src/? | TBD | `ActiveConnectionRegistry::remove` | open (abandoned branch) |
| R.15 | crates/atm-daemon/src/runtime_health.rs | 268 | `mark_sqlite_unavailable` lock-poison | open |
| R.16 | crates/atm-daemon/src/runtime_health.rs | TBD | propagated | open |
| R.17 | crates/atm-daemon/src/runtime_health.rs | TBD | sweep needed | open |

## Fix
At minimum: `tracing::error!("lock poisoned in {}: {}", fn_name, e); return Err(...)`. If recovery is intentional, add: `// SAFETY: poison recovery safe here because <reason>`.

## Fix History
- 2026-05-07: R.14-QA-5 [I] (ActiveConnectionRegistry), R.15-QA-3 [B] (runtime_health.rs:268). R.14 abandoned. R.15-FIX-R3 should address runtime_health.rs.

## QA Round History
- R.14-QA-5: IMPORTANT
- R.15-QA-3: carried (RBP-F001/F002 per QA report)
- R.15-QA-4: carry-forward
