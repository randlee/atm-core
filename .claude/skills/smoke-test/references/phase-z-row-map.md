# Phase Z Frozen Row Map

Rows required by the smoke harness:

- `Z1-001` build approved smoke baseline
- `Z1-002` clean-room daemon/runtime bring-up
- `Z1-003` retained team/member inspection on clean-room baseline
- `Z1-004` empty-mailbox retained CLI surface
- `Z1-005` first clean-room send to config-defined recipient
- `Z1-006` degraded notification after durable send
- `Z1-007` retained CLI validation and recovery guidance
- `Z1-008` copied-state durable baseline bring-up
- `Z1-009` reconcile/runtime retry-visible smoke coverage
- `GRAFT-001` same-host `atm-graft` advisory and unary ICD coverage, run as
  the managed-profile `just smoke graft-hermes` lane
- `FAST-LOG-001` expected happy-path lifecycle/send/read/ack/nudge retained
  events are present
- `FAST-LOG-002` retained logs contain no warnings or errors

Level coverage:

- `fast`
  - `Z1-001`
  - `Z1-002`
  - `Z1-003`
  - `Z1-004`
  - `Z1-005`
  - `FAST-LOG-001`
  - `FAST-LOG-002`
- `normal`
  - everything in `fast`
  - `Z1-007`
- `thorough`
  - every listed fixture row; run `just smoke graft-hermes` alongside it for
    `GRAFT-001`
