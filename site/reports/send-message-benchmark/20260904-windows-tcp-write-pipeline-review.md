# Windows TCP Write-Pipeline Review

## Method

Three independent read-only Luna reviews compared the local write pipeline from
the last recorded Windows TCP/f16/64 PASS, `2973fe2f936c`, to the 1.4.13
candidate, `f39c2236477a`. No build, benchmark run, or source modification was
performed for this review.

The capacity benchmark adds a sender and recipient with no pane or Herdr
metadata and no Graft receiver lease. The delivery classifier therefore selects
`BareCli`; the request's default `Immediate` nudge mode selects a `Steer`
queue-pull dispatch. Plain TCP benchmark ingress is non-peer ingress, so the
router awaits the received-hook future before returning the response.

## Ranked Findings

| Rank | Change | Introduced | Benchmark-path evidence | Windows concern | Status |
| --- | --- | --- | --- | --- | --- |
| P1 | Bare-CLI hook schedules `spawn_blocking`, waits for it, then appends to a process-wide mutex-protected FIFO. | `2517d84f6` | Every newly persisted capacity write is `BareCli` + `Immediate`, selecting `PullPendingReceivedHook`; the local response awaits it. | Thread-pool scheduling and global mutex contention occur on every TCP response. The 32-entry FIFO reaches its drop path during sustained traffic. | High-confidence suspect; needs an A/B measurement. |
| P1 | Search projection and FTS trigger work occur in every unique-message writer transaction. | `1135ef7ce`, changed by `5929160a2` | Each benchmark message is unique and durable. The writer updates the message row, state row, projection, and FTS index in the same transaction. | Longer SQLite WAL transactions and NTFS page writes can increase single-writer occupancy. | Medium-high regression risk; not isolated as the cause. |
| P2 | Local ingress awaits received-hook completion after persistence. | Present after `e2100f0389`; current branch at `storage_and_nudge_router.rs` | TCP smoke ingress is not `Peer`; the await is live. The FIFO dispatch makes it non-empty for the capacity roster. | Couples post-commit work to response latency. | This is the response-path mechanism that exposes P1. |
| P2 | Lease cache synchronization remains on each write. | `1c74392a8`, repaired by `39327974c` | The delivery policy queries the short-lived cache for every write. | Mutex/condition-variable overhead, with an occasional SQLite refresh. | Low-to-medium residual suspect; the previous per-write SQLite lookup is repaired. |
| P3 | HTTP listener retains completed connection tasks until shutdown; each request also reconstructs/parses request data. | `ebca2d6884`, `c1e521aa4`, `f28ee24e8b` and follow-ups | The f16 control creates 9,891 connections and every request copies/parses payload data. | Task bookkeeping and allocator pressure can be more visible on Windows. | Plausible, unproven. |

## Confirmed Repairs Not To Revert

- `39327974c` keeps Graft receiver lease SQLite lookups out of the ordinary
  local write hot path through a short-lived, coalescing cache.
- `40b00b236` keeps queue-marker clearing restricted to `NudgeKind::Queue`.
  Capacity `Immediate` writes are `Steer` and do not perform that transaction.

## Next Discriminator

Do not change the daemon or lower the floor based on this code review. The
first controlled measurement should compare the current TCP/f16/64 path with
the capacity recipient's received-hook dispatch disabled or detached while
retaining durable persistence and response semantics. If throughput materially
recovers, profile the `spawn_blocking` FIFO handoff and its mutex. If it does
not, measure writer transaction time and FTS/WAL work next.
