# AN.15 checked-emission campaign provenance

- Campaign target: `atm-template-checked-emission`
- Campaign seed: `20260814`
- Candidate worktree/ref: `feature/an15-adversarial-fuzzing` at
  `79c3577e331361a498be90761ffd915940683f43`
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
