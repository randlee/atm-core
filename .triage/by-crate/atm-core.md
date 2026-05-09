# atm-core — Active Findings Index

Crate path: `crates/atm-core/`

## Open Findings

None currently tracked. Update when triage agents surface findings in this crate.

## Notes
- atm-core is the boundary/protocol crate — findings here tend to be API/type-level (RBP newtype violations, RULE-008 test constants)
- Watch for: `&str` parameter surfaces that should be typed newtypes (`&TeamName`, `&AgentName`)
