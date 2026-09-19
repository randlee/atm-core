---
title: Doctor And Log
audience: end-user
reviewed_for_release: 1.6.0
---

# Doctor And Log

ATM exposes operator diagnostics through the supported CLI surfaces.

## Doctor

Use `atm doctor` when you need to confirm that ATM configuration, runtime
state, and daemon-related surfaces are healthy.

Common doctor usage:

```bash
export ATM_TEAM=atm-dev

atm doctor --team atm-dev
atm doctor --team atm-dev --json
```

High-signal doctor output usually tells you one of three things:

- caller/team/config resolution is healthy
- the daemon/runtime path is healthy
- ATM found a configuration, connectivity, or storage problem that needs a
  supported recovery step

### JSON Sections

`atm doctor --json` is the supported machine-readable diagnostic surface. In
addition to caller and daemon context, current releases can include:

- `reader_lanes` for the shared read pool's effective capacity and live queue,
  saturation, and in-flight metrics
- `graft_receivers.receivers` for registered graft receiver leases
- `herdr` for whether a Herdr backend is configured, its breaker, and one
  endpoint record per default or named Herdr session

Each `herdr.endpoints[]` record includes its `session`, `provenance`,
`transport`, `endpoint`, `state`, `remedy`, `capabilities.live_handoff`, and
the configured roster `members`; unavailable details are represented by the
documented nullable fields. Use the endpoint `state` and `remedy` rather than
guessing from a daemon log. See [Herdr Integration](./herdr.md) for the
endpoint and unique-name model.

## Log

Use the ATM log surface when you need structured evidence for a failure,
warning, or unexpected runtime path.

The retained CLI surface is:

- `atm log snapshot`
- `atm log filter`
- `atm log tail`

Examples:

```bash
export ATM_TEAM=atm-dev
export ATM_IDENTITY=arch-ctm

atm log snapshot --limit 20
atm log filter --level warn --match command=send
atm log tail --level error
```

Retained ATM logs are host-scoped, not selected by workspace `ATM_HOME` and not
stored in the installed document tree. The ordinary operator expectation is
`~/.atm/logs/atm.log.jsonl`; `ATM_LOG_DIR` is the supported absolute-directory
override when the retained log must live somewhere else. `atm doctor --json`
reports the active log path for the current host.

## Separation Of Concerns

These diagnostics are supported operator-facing entrypoints. They are preferred
over ad hoc local-state inspection.

If a command fails, prefer:

1. `atm doctor`
2. `atm log snapshot` or `atm log filter`
3. the recovery guidance in [Troubleshooting](./troubleshooting.md)

Additional runnable examples live in [examples/diagnostics/](./examples/diagnostics/).

For general recovery steps, continue to [Troubleshooting](./troubleshooting.md).

Return to the [ATM User Guide](./README.md).
