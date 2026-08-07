# Smoke Level Matrix

Run every level through the canonical operator entry point: `just smoke`,
`just smoke fast`, or `just smoke thorough`. The Python modules under
`scripts/smoke/` are internal implementations, not alternate commands.

## `fast`

- clean-room happy path only
- prove daemon bring-up, team setup, `doctor`, both `atm send` modes,
  `atm read`, `atm ack`, nudge-visible flow, and clean shutdown
- fail on missing retained lifecycle/send/read/ack/nudge events or any
  warning/error output

## `normal`

- includes `fast`
- adds broader retained/admin/operator coverage
- root-cause every deviation from expected behavior

## `thorough`

- includes `normal`
- covers every frozen smoke row plus every CLI happy path and common error path
- includes one real same-host `atm-graft` advisory plus unary ICD lane
- row-by-row PASS / FAIL / SKIP output
