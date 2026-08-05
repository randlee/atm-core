# AK.10 post-write router source-to-boundary comparison

This record closes `AK5-BOUNDARY-DRIFT-001` against AK.9 commit `2d358f7e`.
It compares the active implementation directly with
[`post-write-router.toml`](../../../boundaries/atm-daemon/post-write-router.toml).

| Boundary field | `peer_delivery_router.rs` source | AK.10 record |
| --- | --- | --- |
| `io_owns` | `dispatch` classifies the completed write; `signal_local_post_write` registers and signals local post-commit work; the cache-disabled host-qualified branch calls `send_peer_http_batch`. | `post_commit_route_classification`, `local_post_commit_signal`, and narrowly-scoped `configured_direct_peer_http_batch_delivery`. |
| `io_forbidden` | The router imports no SQLite, DNS, TLS, hook, graft, or nudge-emission API. Local nudge work is only identified and signalled; it is executed by the separate post-commit worker. | `sqlite`, `dns`, `tls`, `hook_execution`, `graft_delivery`, and `nudge_emission`. |
| Request contract | `dispatch` accepts one already committed `MessageRecord`. | `committed MessageRecord`. |
| Response contract | Peer-receipt and hostless branches signal `PostCommitWorkKey::LocalNudge` and return `Ok(())`; a host-qualified branch completes one configured direct batch delivery or optional scheduler handoff. | Local signal or configured direct batch-delivery confirmation. |
| Error contract | Host-qualified endpoint/config lookup and direct delivery propagate `AtmError::remote_delivery_unconfirmed`; local signal paths return success. | `AtmError::RemoteDeliveryUnconfirmed` is the only declared router error. |
| Three-route behavior | (1) `is_peer_receipt` signals local work and returns; (2) a host-qualified origin selects `send_peer_http_batch` directly when cache-disabled, or `deliver_or_queue` through the optional scheduler, and never calls `signal_local_post_write`; (3) the hostless tail signals local work and returns. | The boundary note states the same three outcomes. |

The executable source guard
`peer_delivery_router::tests::source_guard_locks_the_three_post_write_route_outcomes`
locks this comparison. Existing focused integration tests cover the actual
cache-disabled singleton array confirmation and typed unconfirmed-delivery
failure behavior.
