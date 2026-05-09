# CI-WIN-001: Ungated Unix-only imports and symbols

## Pattern
```
use std::os::unix
use std::fs::OpenOptions
use std::time::Duration   # when in unix-only context
DaemonSupervisor
LocalSocketClientTransport
SAME_HOST_REQUEST_DEADLINE
AUTO_START_PUBLISH_TIMEOUT
\.display()              # Path::display() — Windows clippy warns
```

## Crates Affected
- atm (crates/atm/src/composition.rs)
- atm-daemon (crates/atm-daemon/src/composition.rs)

## Sprint Origin
R.13 (first reported as CI failure)

## Status
fixed-partial

## Description
Unix-only symbols (imports, constants, methods, transport types) visible to Windows compiler without `#[cfg(unix)]` / `#[cfg(not(unix))]` gates. Windows clippy raises `unused-imports`, `dead-code`, or compile errors. All unix-only code paths must be gated.

Fix pattern:
- Imports: `#[cfg(unix)] use ...`
- Constants: `#[cfg(unix)] const NAME: Type = value;`
- Struct fields/methods: `#[cfg_attr(not(unix), allow(dead_code))]`
- Test modules with unix-only deps: `#[cfg(unix)]` on the `mod tests` block

## Occurrences
| Branch | File | Line | Symbol | Fixed |
|--------|------|------|--------|-------|
| R.13 (e91f97f) | crates/atm/src/composition.rs | 5 | Duration import | fixed |
| R.13 (e91f97f) | crates/atm/src/composition.rs | 33-34 | SAME_HOST_REQUEST_DEADLINE, AUTO_START_PUBLISH_TIMEOUT | fixed |
| R.13 (e91f97f) | crates/atm/src/composition.rs | 628-629 | DaemonSupervisor, LocalSocketClientTransport test imports | fixed |
| R.13 (b36c946) | crates/atm-daemon/src/composition.rs | 7 | OpenOptions | fixed |
| R.15 | various | TBD | CI-WIN reported in QA-4 | open |
| R.16 | various | TBD | propagated — sweep needed | open |
| R.17 | various | TBD | propagated — sweep needed | open |

## Fix History
- 2026-05-07: R.13 composition.rs fixed at b36c946 (OpenOptions) and e91f97f (atm/composition.rs constants + test imports)
- R.15-QA-4 reported new CI-WIN failures — different files, not yet traced

## QA Round History
- R.13-QA-2: CI-BLOCK
- R.13-QA-5: fixed at b36c946
- R.13-CI-FIX-R2: atm/composition.rs fixed at e91f97f
- R.14-QA-5: CI-WIN-NEW-001 carry-forward (same symbols, different branch)
- R.15-QA-4: CI-WIN new — scope TBD
