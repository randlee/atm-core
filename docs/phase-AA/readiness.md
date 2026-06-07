# Phase AA Readiness

## Goal

Track the accepted closure state for the daemon simplification line that
removes concrete SQLite knowledge from `atm-daemon`.

Authoritative supporting inventory:
- `docs/phase-AA/issues.md`

## Sprint Status

| Sprint | Status | Branch | Worktree | Closure Gate |
| --- | --- | --- | --- | --- |
| `AA.0` | `complete` | `feature/pAA-s0-daemon-architecture-restatement` | `../atm-core-worktrees/feature/pAA-s0-daemon-architecture-restatement` | daemon role, doctor aggregation model, and state-machine inventory documented and accepted |
| `AA.1` | `complete` | `feature/pAA-s1-subsystem-doctor-traits` | `../atm-core-worktrees/feature/pAA-s1-subsystem-doctor-traits` | subsystem-owned capability/doctor traits and shared diagnostic DTOs land in the governing docs and code |
| `AA.2` | `complete` | `feature/pAA-s2-atm-runtime-composition-transfer` | `../atm-core-worktrees/feature/pAA-s2-atm-runtime-composition-transfer` | `atm-runtime` owns concrete SQLite/runtime assembly; `atm-daemon` stops constructing SQLite boundaries in production; target-state runtime boundary is frozen even though SQLite TOML relock waits for `AA.5` |
| `AA.3` | `complete` | `feature/pAA-s3-direct-doctor-and-runtime-health-split` | `../atm-core-worktrees/feature/pAA-s3-direct-doctor-and-runtime-health-split` | `atm doctor` regains direct local store diagnostics; daemon aggregates injected subsystem reports plus daemon-owned runtime state without backend-specific diagnosis logic |
| `AA.4` | `complete` | `feature/pAA-s4-delete-daemon-sqlite-leaks` | `../atm-core-worktrees/feature/pAA-s4-delete-daemon-sqlite-leaks` | remaining SQLite observability, replay, and test-support leaks are removed from `atm-daemon` |
| `AA.5` | `complete` | `feature/pAA-s5-boundary-relock-and-permanent-enforcement` | `../atm-core-worktrees/feature/pAA-s5-boundary-relock-and-permanent-enforcement` | daemon-to-SQLite edge is forbidden again, all boundary TOMLs agree on that policy, and a second enforcement layer exists beyond TOML lint |
| `AA.6` | `complete` | `feature/pAA-s6-obs-upgrade` | `../atm-core-worktrees/feature/pAA-s6-obs-upgrade` | ATM builds and validates on `sc-observability` / `sc-observability-types` `1.2.0` with the queue-backed logger API, retained-log policy field migration, and updated health projection |
| `AA.7` | `complete` | `feature/pAA-s7-atm-architecture-crate` | `../atm-core-worktrees/feature/pAA-s7-atm-architecture-crate` | Rust boundary enforcement crate lands, Python boundary scripts are removed, and `cargo test -p atm-architecture` becomes the sole code-driven boundary guard |
| `AA.8` | `complete` | `feature/pAA-s8-claude-schema-contract` | `../atm-core-worktrees/feature/pAA-s8-claude-schema-contract` | current Claude Code inbox schema is frozen from real `team-lead -> quality-mgr` samples and all docs/models stop calling the current JSON-array inbox shape legacy |
| `AA.9` | `planned` | `feature/pAA-s9-claude-inbox-primary-path` | `../atm-core-worktrees/feature/pAA-s9-claude-inbox-primary-path` | the normal retained append path treats the current Claude inbox file shape as supported primary behavior rather than degraded legacy behavior |
| `AA.10` | `planned` | `feature/pAA-s10-remove-historical-atm-json` | `../atm-core-worktrees/feature/pAA-s10-remove-historical-atm-json` | historical ATM-owned JSON schema variants stop being the active 1.2 contract while legal ATM additive derivatives remain accepted on read |
| `AA.11` | `planned` | `feature/pAA-s11-delete-sqlite-legacy-compat` | `../atm-core-worktrees/feature/pAA-s11-delete-sqlite-legacy-compat` | pre-production SQLite compatibility scaffolding such as `legacy_message_id` support is removed from the active 1.2 runtime line unless explicitly reapproved |
| `AA.12` | `planned` | `feature/pAA-s12-malformed-claude-inbox-recovery` | `../atm-core-worktrees/feature/pAA-s12-malformed-claude-inbox-recovery` | malformed Claude inbox content degrades and salvages recoverable messages instead of aborting the entire inbox surface where segmented valid records still exist |

## Phase Exit Criteria

`Phase AA` is not complete until all of the following are true:

- `crates/atm-daemon/Cargo.toml` has no direct `atm-rusqlite` dependency
- no `atm_rusqlite::*` references remain in daemon production code
- direct local store diagnostics exist behind subsystem-owned doctor traits
- storage behavior is expressed through small behavior-named capability traits,
  not backend-shaped interfaces
- daemon doctor aggregation does not inspect SQLite internals directly and only
  compares subsystem reports at the aggregate level
- the boundary TOMLs forbid the daemon-to-SQLite edge again
- a repository-enforced dependency-boundary test or equivalent guard fails
  whenever that edge reappears
- `cargo test --package atm-architecture` is the required second guard for
  review-time policy widening detection
- a `boundary-guard` QA agent reviews both plans and phase-ending reviews
  and flags boundary-policy widening before closure
- ATM has completed the `sc-observability` / `sc-observability-types` `1.2.0`
  upgrade and no remaining migration blocker from the deprecated
  `Logger::emit()` path or the old retained-log policy field surface remains
- the current Claude Code inbox JSON schema is the explicitly documented
  primary shared inbox contract and is proven against real fixture-backed
  `team-lead -> quality-mgr` samples
- the normal retained runtime path does not classify the current array-backed
  Claude inbox file shape as legacy or degraded-only behavior
- no active 1.2 documentation or runtime path presents historical ATM-owned
  inbox JSON schema variants such as top-level ATM fields or `metadata.atm.*`
  as the primary or forward-write contract, while legal ATM 1.1 additive
  derivatives still parse successfully on read
- one malformed shared-inbox fragment cannot hide unrelated valid messages when
  the valid messages are still segmentable from the same Claude inbox file
- no active 1.2 runtime path depends on pre-production SQLite compatibility
  scaffolding such as `legacy_message_id`, unless an explicit exception is
  recorded in this readiness file
- `docs/phase-AA/issues.md` has no open issue whose planned closure sprint is
  still incomplete
