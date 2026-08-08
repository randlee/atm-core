# Smoke Report Schema

Canonical payload fields:

- `level`
- `timestamp`
- `binary_sha`
- `duration_secs`
- `status`
- `rows`
- `summary`

Live hardware smoke additionally records:

- `feature`
- `platform`
- `host`
- `run_id`
- `status`
- `cases`

Every row record may carry:

- `id`
- `flow`
- `verdict`
- `notes`
- `observed_behavior`
- `expected_behavior`
- `likely_root_cause`
- `artifact_pointer`

## Live hardware artifact layout

Every live `just smoke localhost`, `just smoke local-ip`, or cross-host run
owns one directory, following the fuzz-report principle of self-contained
evidence:

`site/reports/smoke/<platform>/<host>/<run-id>-pid<PID>-<feature>/`

For example, a Windows local-IP result could be:

`site/reports/smoke/windows/cwin/20260808T001234567890Z-pid4242-local-ip/`

That directory contains `<feature>.json`, `<feature>.html`, `index.html`, and
the XHTML evidence panels. Platform and host are present in both the directory
path and JSON payload. There are no shared/latest smoke artifacts and no smoke
files directly in `site/` or `site/reports/`.
