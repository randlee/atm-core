# RULE-005: Duplicate struct definition across crate boundaries (resolved)

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
closed — resolved by deleting the retired peer transport and its replay DTO.

## Description
The duplicate existed only to support the retired peer-transport replay path. That path and its DTO were deleted; the local-IPC daemon has no replay-store contract.

## Occurrences
| Branch | File | Line | Snippet | Fixed |
|--------|------|------|---------|-------|
| pre-reset | retired peer transport | n/a | deleted with its replay DTO | resolved |

## Fix
Do not reintroduce replay persistence for peer delivery. The architecture gate rejects retired peer/replay constructs.

## Fix History
- 2026-05-07: First reported R.16-QA-4 [B] on 7413d20. Carry-forward to R.17 expected. Fix target: R.17 (highest).

## QA Round History
- R.16-QA-4: BLOCKING
