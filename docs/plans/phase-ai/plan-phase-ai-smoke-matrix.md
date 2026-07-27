# Phase AI smoke matrix

This matrix records repeatable branch-daemon proof obligations. It does not
replace the cross-host release smoke runbook.

| Sprint | Scenario | Required proof | Evidence |
| --- | --- | --- | --- |
| AI.24 | Same-host advertised-IP ACK | A release-built daemon receives an ack-required write over its advertised IPv4 peer interface; `atm ack` produces one readable reply with `acknowledges_message_id` and one recipient nudge after persistence. The source pending ACK becomes acknowledged through the canonical receive-side write. | Sanitized daemon log containing `peer_duplicate_write_skipped`, reply ULID readable through `atm read`, recipient nudge, daemon PID, and matching client/daemon `1.3.2-beta-24` doctor output. |

The AI.24 row is a same-host prerequisite. Mocked routers, direct dispatcher
calls, and mocked nudge sinks are not release evidence for this row.
