# State Machine And Integration Boundary Guidelines

This document defines review rules for state machines and client-facing
integration layers such as graft.

## State machine rule

Every state machine must be as simple as possible.

If a state machine exists only because the code has two paths that should be
one path, the split is the bug.

## Minimal state rule

A state machine should:

- have one clear owner
- encode only necessary states
- avoid duplicated transition logic
- avoid parallel top-level variants for the same business event

## Integration rule

Client integration features must sit behind narrow ports.

The daemon or core runtime should not own integration-specific workflow state
unless that state is part of the retained product contract.

## What to flag

Flag a finding when:

- send and ack are modeled as separate top-level workflows instead of one
  message path with data differences
- transport and business logic each maintain their own state machine for the
  same event
- a client integration requires daemon-internal knowledge of its private
  workflow vocabulary
- a component bypasses an existing observability or notification port and uses
  a side channel instead
- proof-only workflow logic survives in production code

## What to prefer

- one canonical workflow
- one internal mode/field instead of parallel pipelines
- one explicit state owner
- simple state transitions that can be enumerated and tested directly
- deletion of orchestration glue when the underlying path split is removed
