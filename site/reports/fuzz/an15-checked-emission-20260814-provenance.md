# AN.15 checked-emission campaign provenance

- Campaign target: `atm-template-checked-emission`
- Campaign seed: `20260814`
- Campaign ID: `an15-checked-emission-20260814`
- Candidate worktree/ref and final CI commit: `feature/an15-adversarial-fuzzing`
  at `992a0828ef15fa021d46ee32dbea1adf865f21dd`
- AN.14 baseline ref: `9c3d9e2a40535833bdf8c1a46a6f7eb1de88b44f`
- Fixed workers: `shape-probe`, `template-probe`, `boundary-probe`, and
  `differential-probe`; each executed 100 seeded vectors with a 120-second
  individual timeout.
- Result: all four workers succeeded; no candidates, confirmed bugs, or
  unassigned safety investigations were produced.

The campaign used the locked crates.io sources rather than an unpublished
revision or path override:

| Crate | Version | Cargo.lock checksum |
| --- | --- | --- |
| `sc-composer` | `1.4.1` | `4415ff74a7f91a7505a7c9fc464908ed5e0e684d2648b5d731e0533c371edb2c` |
| `sc-sha` | `1.4.1` | `01502b8bda56eef5c2f445a88396d75cc223c8ce91709ac007dbb81f40e577ba` |

`cargo tree -p atm-template-sc-compose -e normal` resolved
`sc-composer v1.4.1` and `sc-sha v1.4.1`. The campaign runner invokes only
closed-over `cargo test` test seams. It does not invoke the `sc-compose` CLI,
accept a caller-supplied command, or edit production code.

## Upstream release and checked-render evidence

The campaign report's structured `provenance` object points to both retained
source snapshots below. They are kept adjacent to the report so a reviewer can
validate this gate without relying on a mutable external page.

| Evidence | Retained path | Verified fact | SHA-256 |
| --- | --- | --- | --- |
| crates.io release | `an15-sc-compose-1.4.1-crates-io.json` | `sc-compose` `1.4.1`, published 2026-08-14T01:16:45Z, not yanked; registry checksum `bb3111c4fc261f4aff1e341bd76475ce13524b9b768021ceb79da9761e31dd05` | `91732ebd15d04dd7d6d66e73eb5f57bfe416bfe41df159e6bf9c19fa8527c199` |
| upstream checked-render issue | `an15-sc-compose-448-github.json` | [randlee/sc-compose#448](https://github.com/randlee/sc-compose/issues/448) closed as completed 2026-08-13T20:44:19Z | `5409e52f3cd4df53fc81f02ca83c5ab98150f965cb6c151277eb077f9171385c` |

The crates.io snapshot was retrieved from
`https://crates.io/api/v1/crates/sc-compose/1.4.1`; `cargo info
sc-compose@1.4.1` independently resolved the same release from crates.io.
