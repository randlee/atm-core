# bo-10 — atm-bd-lite hooks (ready to open as a PR against atm-bd-orchestration)

Built 2026-11-01 from the hook code in `sprint-bo-10-lite-hooks.md` and the bo-9 stub in `sprint-bo-9-types-020.md`.

| check | result |
|---|---|
| pytest `tests/lite` | 47 passed — one test per Transitions row and per Errors row of `hooks/*.md` |
| pyright strict against `typings/atm_bd_types/__init__.pyi` | 0 errors |
| gate deterministic (ANC-052/064/065/066/070) | pass on all three hooks (`anchor-runs/hooks/*.json`) |
| gate Jev half (ANC-053/054) | not run here: needs `TYPESAFE_API_KEY`; run `atm-bd-lite-gate.py` on each hook before opening the PR |
| total lite Python (ANC-077, proposed) | 171 lines |

Layout is the installed layout: `skills/atm-bd-lite/scripts/` holds the hooks; `typings/` the stub; `tests/lite/` the tests
and `fake_atm_bd_types.py`, a test-only in-memory implementation of the stub so the hooks execute before the Rust crate
is published. Delete the fake when `atm-bd-types==0.2.0` is on PyPI and the tests run against the wheel.

To open the PR: copy `skills/`, `typings/`, `tests/`, `pyrightconfig.json` into atm-bd-orchestration on branch
`sprint/bo-10-lite-hooks`, run `pytest tests/lite`, `pyright`, and the gate with the key; attach the three run JSONs.
