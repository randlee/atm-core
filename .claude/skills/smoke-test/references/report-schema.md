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

Tracked latest markdown artifacts:

- `reports/smoke/smoke-fast.md`
- `reports/smoke/smoke.md`
- `reports/smoke/smoke-thorough.md`

Timestamped markdown artifacts:

- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke-fast.md`
- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke.md`
- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke-thorough.md`

Timestamped JSON artifacts:

- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke-fast.json`
- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke.json`
- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke-thorough.json`
