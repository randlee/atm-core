# RULE-005: Duplicate struct definition across crate boundaries

## Pattern
```
RemoteReplayStateRecord
pub struct.*Record.*\{
pub\(crate\) struct.*Record.*\{
```

## Crates Affected
- atm-daemon (peer_transport.rs)
- atm-rusqlite (lib.rs)

## Sprint Origin
R.16 (first reported R.16-QA-4)

## Status
open

## Description
`RemoteReplayStateRecord` defined in both `crates/atm-daemon/src/peer_transport.rs:74` (pub(crate)) and `crates/atm-rusqlite/src/lib.rs:30` (pub). Duplicate struct definitions across crate boundaries violate RULE-005 (single source of truth). The canonical definition must live in one crate only. Per ARCH-001 boundary rule, peer_transport.rs must not directly import from atm-rusqlite; the struct must be referenced via injected trait/type through the composition root.

## Occurrences
| Branch | File | Line | Snippet | Fixed |
|--------|------|------|---------|-------|
| R.16 | crates/atm-daemon/src/peer_transport.rs | 74 | `pub(crate) struct RemoteReplayStateRecord` | open |
| R.16 | crates/atm-rusqlite/src/lib.rs | 30 | `pub struct RemoteReplayStateRecord` | open (canonical) |
| R.17 | crates/atm-daemon/src/peer_transport.rs | TBD | sweep needed | open |
| R.17 | crates/atm-rusqlite/src/lib.rs | TBD | sweep needed | open |

## Fix
Keep canonical definition in atm-rusqlite/src/lib.rs (persistence crate). Remove duplicate from peer_transport.rs. peer_transport.rs references the type via injected trait or re-export through the composition root — never via direct `use atm_rusqlite::...` import.

## Fix History
- 2026-05-07: First reported R.16-QA-4 [B] on 7413d20. Carry-forward to R.17 expected. Fix target: R.17 (highest).

## QA Round History
- R.16-QA-4: BLOCKING
