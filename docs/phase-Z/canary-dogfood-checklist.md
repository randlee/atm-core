# Phase Z Canary And Dogfood Checklist

## Purpose

Authoritative operator-facing checklist for `Z.3`.

## Record Schema

The checklist must freeze:

- the `atm-dev` participant list
- the approved binary baseline under evaluation
- the reporting path used for operator findings
- the operator flows and recovery behaviors each participant is expected to
  exercise

Each checklist row must record:

- `participant`
- `operator_flow`
- `expected_behavior`
- `recovery_behavior`
- `verdict`
- `notes`

## Rules

- this checklist is frozen at the start of `Z.3`
- every checklist row must record one final verdict before `Z.3` closes; any
  row left without a final verdict is blocking for the sprint
- operator reports that do not map back to a checklist row must be added as
  explicit findings with notes explaining the extra coverage
