# Smoke Report Schema

Canonical payload fields:

- `level`
- `timestamp`
- `binary_sha`
- `duration_secs`
- `status`
- `rows`
- `summary`

Every row record may carry:

- `id`
- `flow`
- `verdict`
- `notes`
- `observed_behavior`
- `expected_behavior`
- `likely_root_cause`
- `artifact_pointer`

## Artifact Rules

Smoke reports are published to the site, matching the
`site/reports/send-message-benchmark/` benchmark-harness convention: a flat
per-level directory, every file timestamp- and host-labeled, and no fixed
"latest" filename. Different machines running smoke tests concurrently never
collide, because each run gets its own unique `<timestamp>-<host_label>-*`
name.

Per-level evidence directories:

- `site/reports/smoke-fast/`
- `site/reports/smoke/` (the `normal` level)
- `site/reports/smoke-thorough/`

Per-run artifacts inside each evidence directory (`<timestamp>` uses
`YYYY-MM-DD-HH-MM-SS`, `<host_label>` is the sanitized machine hostname, and
`<slug>` is `smoke-fast` / `smoke` / `smoke-thorough`):

- `site/reports/<slug>/<timestamp>-<host_label>-<slug>.md`
- `site/reports/<slug>/<timestamp>-<host_label>-<slug>.json`
- `site/reports/<slug>/<timestamp>-<host_label>-<slug>.envelope.json`

Root-level discovery page per level, generated from the evidence directory and
wired into `site/reports/index.html` via `just reports-index`:

- `site/reports/smoke-fast.html`
- `site/reports/smoke.html`
- `site/reports/smoke-thorough.html`
