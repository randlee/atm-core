# /// script
# dependencies = ["jev-check"]
# [tool.uv.sources]
# jev-check = { path = "../../../../design/packages/jev-check", editable = true }
# ///
"""tune-round.py <questions.json> <out.json> [repeats]   (run from docs/plans/phase-bo)

One tuning round: every gate fixture, Jev `repeats` times (byte-identical requests) and Haiku once, scored
against gate-fixtures/expected.json (the frozen oracle: listed ids are expected no, all others yes).
Each cell: expected, Jev choices + confidences, Haiku choice + confidence, ok (every Jev run matches), and
the Haiku session id to `ask` about it."""
import importlib.util, json, sys
from pathlib import Path
from jev_check import run_request

spec = importlib.util.spec_from_file_location("g", "design/lite-code/atm-bd-lite-gate.py")
g = importlib.util.module_from_spec(spec); spec.loader.exec_module(g)
g.QUESTIONS = Path(sys.argv[1])
repeats = int(sys.argv[3]) if len(sys.argv) > 3 else 2
fixtures = Path("design/lite-code/gate-fixtures")
oracle = json.loads((fixtures / "expected.json").read_text())
spec_md = Path("hooks/task-close.md").read_text()
out = {"questions": sys.argv[1], "repeats": repeats, "fixtures": {}}
for name, exp in oracle.items():
    src = (fixtures / f"{name}.py").read_text()
    f = g.facts(src)
    qs = g.questions(f, g.table(spec_md, "Transitions"))
    state = {"hook": src, "spec": spec_md}
    jev = [run_request(name, "worktree", "jev", state, qs) for _ in range(repeats)]
    haiku = run_request(name, "worktree", "haiku", state, qs)
    cells = {}
    for k in qs:
        want = "no" if k in exp["expected_no"] else "yes"
        cells[k] = {"expected": want, "jev": [[r.answers[k]["choice"], r.answers[k]["confidence"]] for r in jev],
                    "haiku": [haiku.answers[k]["choice"], haiku.answers[k]["confidence"]],
                    "ok": all(r.answers[k]["choice"] == want for r in jev)}
    out["fixtures"][name] = {"haiku_session": haiku.session_id, "haiku_input": vars(haiku.input),
                             "jev_sessions": [r.session_id for r in jev],
                             "ok": f"{sum(c['ok'] for c in cells.values())}/{len(cells)}", "cells": cells}
Path(sys.argv[2]).write_text(json.dumps(out, indent=1) + "\n")
for name, fx in out["fixtures"].items():
    print(name, fx["ok"], "haiku", fx["haiku_session"])
    for k, c in fx["cells"].items():
        weak = min(conf for _, conf in c["jev"]) < 0.5
        if not c["ok"] or weak or c["haiku"][0] != c["expected"]:
            print(f"  {'FAIL' if not c['ok'] else 'weak'} {k} want {c['expected']} jev {c['jev']} haiku {c['haiku']}")
