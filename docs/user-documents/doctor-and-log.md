---
title: Doctor And Log
audience: end-user
reviewed_for_release: 1.3.0
---

# Doctor And Log

ATM exposes operator diagnostics through the supported CLI surfaces.

## Doctor

Use `atm doctor` when you need to confirm that ATM configuration, runtime
state, and daemon-related surfaces are healthy.

## Log

Use the ATM log surface when you need structured evidence for a failure,
warning, or unexpected runtime path.

## Separation Of Concerns

These diagnostics are supported operator-facing entrypoints. They are preferred
over ad hoc local-state inspection.

For general recovery steps, continue to [Troubleshooting](./troubleshooting.md).

Return to the [ATM User Guide](./README.md).
