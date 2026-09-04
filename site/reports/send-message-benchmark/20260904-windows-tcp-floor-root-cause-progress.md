# Windows TCP Floor Root-Cause Progress

## Scope

This report compares retained Windows benchmark evidence only. It does not
build or run a historical revision, and it does not change `baselines.json`.

## Comparable TCP Evidence

All rows below use Windows `tcp`, 16 frames per connection, and the historical
64-worker runner shape.

| Timestamp (UTC) | Revision | p50 msg/s | Interpretation |
| --- | --- | ---: | --- |
| 2026-08-01 21:13 | `2973fe2f936c` | 8,793.91 | Last recorded PASS and source of the current floor. |
| 2026-08-01 22:48 | `fd8dd58e04a8` | 8,636.26 | Already below the seeded floor. |
| 2026-08-21 02:52 | `78ec6008c03d` | 6,581.30 | Latest retained pre-candidate comparable evidence. |
| 2026-09-04 16:14 | `f39c2236477a` | 7,928.89 | 1.4.13 candidate control, retained in the adjacent JSON artifact. |

The candidate remains 9.8% below the historical seed, but is 20.5% above the
latest comparable pre-candidate record. The retained evidence therefore does
not support attributing the remaining floor miss to the 1.4.13 candidate.

## Known Pipeline Regression And Current State

Canonical finding `RRG-INGRESS-ADMIT-001` records an earlier, independently
root-caused throughput regression after the historical seed:

1. AQ graft endpoint delivery added a blocking control-path SQLite borrow to
   each local write (`334e5ca89` / `1c74392a8`).
2. Bare-CLI queue-pull handling then cleared a marker for each Immediate write,
   adding another control-path transaction and SQLite WAL contention.
3. The candidate contains the repairs: `39327974c` caches graft receiver lease
   resolution off the write path, and `40b00b236` limits marker clearing to
   `NudgeKind::Queue`.

The current source confirms the Immediate-write guard remains present in
`crates/atm-daemon-bootstrap/src/received_hook_selector.rs`.

## Remaining Work

The cause of the residual 9.8% difference from the one historical PASS has
not been isolated. The next valid discriminator is performance profiling of
the current TCP/f16/64 path on the Windows benchmark account, correlated with
the retained evidence. No runtime change is justified from the evidence in
this report alone.
