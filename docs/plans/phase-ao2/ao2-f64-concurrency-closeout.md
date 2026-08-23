---
phase: AO2
scope: f64-diagnostic-closeout
integration_branch: integrate/phase-ao2
integration_revision: dbdfa43cd717c487e1c9b2b15354fcbe312bc1c9
status: root_caused-no-production-change
---

# AO2 f64 concurrency-collapse closeout

## Decision

The apparent f64 throughput collapse is a benchmark-client workload-shape
effect, not a TLS regression or a reason to alter the shipped Tokio/Axum
daemon, writer, public HTTP contract, or TLS wrapper.  The accepted AO2 f8
workload remains the performance metric.  No production change is warranted
for f64 diagnostics.

## Evidence

All figures below are M5 dedicated-account runs with the normal isolated
snapshot/restore lifecycle.  The f64 measurements use 1,000 requests per
interval and a requested worker cap of 512; they are diagnostics, not the
accepted f8 workload.

| Mode | Revision | Frames / connection | p50 msg/s | Restore proof |
| --- | --- | ---: | ---: | --- |
| Plaintext TCP | `ff9329dc1` | 8 | 18,001.81 | byte-identical |
| Plaintext TCP | `ff9329dc1` | 64 | 2,959.55 | byte-identical |
| mTLS TCP | `ff9329dc1` | 8 | 17,951.68 | byte-identical |
| mTLS TCP | `dbdfa43cd` | 64 | 2,993.98 | byte-identical |

The mTLS f64 run on merged `dbdfa43cd` recorded 16 connections and 16
connection workers; its clean-baseline, restored-clean-baseline, and
restored-live-database SHA-256 values were identical.  The equivalent
plaintext f64 run on the immediately preceding harness-only candidate had the
same 16/16 topology and a 2,959.55 msg/s p50.  The nearly identical f64 values
across plaintext and mTLS rule out TLS as the cause of the diagnostic shape.

## Root cause

The runner intentionally derives the number of client connections from the
frame count:

```text
connections = ceil(requests_per_interval / frames_per_connection)
```

For the fixed 1,000-request diagnostic interval this yields 125 connections
at f8 and only 16 connections at f64.  Each connection uses the bounded
eight-request HTTP/1.1 pipeline in `submit_connection`: it sends at most eight
requests, drains all eight matching responses, then sends the next group.

Consequently, the maximum simultaneous request count is materially different:

| Profile | Connections | Maximum in-flight requests |
| --- | ---: | ---: |
| f8 | 125 | 1,000 |
| f64 | 16 | 128 |

The f64 measurement therefore supplies roughly one eighth of the request
concurrency of f8, then serializes the remaining per-connection groups behind
response drains.  Its roughly 3k msg/s result measures that lower-concurrency
client geometry; it does not measure the same pipeline under a larger framing
unit.

## Boundary and follow-up

- The f64 diagnostic confirms no TLS-only regression: plaintext and mTLS
  collapse together.
- It does not identify a server-side bottleneck at f64, because the harness
  deliberately changes its offered concurrency before the daemon receives the
  traffic.  No server, writer, router, TLS, or transport change is justified
  by this diagnostic alone.
- AO2 acceptance stays on the immutable f8 workload, whose matching physical
  results are within the documented five-percent tolerance of the 22.5k TCP
  target.
- A future product decision may define a separate high-frame workload with a
  fixed offered-concurrency contract.  That would be a new benchmark contract
  and sprint, not an f64 tuning change disguised as an AO2 parity fix.
