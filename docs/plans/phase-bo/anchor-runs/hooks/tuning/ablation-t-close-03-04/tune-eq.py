# /// script
# dependencies = ["jev-check"]
# [tool.uv.sources]
# jev-check = { path = "/Users/randlee/github/atm-core-worktrees/plan/phase-bo-replan/docs/plans/phase-bo/design/packages/jev-check", editable = true }
# ///
"""tune-eq.py <questions.json> <fixture>...  (run from docs/plans/phase-bo)

Same request on Jev, Haiku and Luna through jev-check run_request (API-direct). Records each side's input
record; agreement is counted only when every side is equal."""
import dataclasses, importlib.util, json, sys
from pathlib import Path
from jev_check import JevCheckError, run_request

spec = importlib.util.spec_from_file_location("g", "design/lite-code/atm-bd-lite-gate.py")
g = importlib.util.module_from_spec(spec); spec.loader.exec_module(g)
g.QUESTIONS = Path(sys.argv[1])
out = {"questions": sys.argv[1]}
for name in sys.argv[2:]:
    src = Path(f"design/lite-code/gate-fixtures/{name}.py").read_text(); spec_md = Path("hooks/task-close.md").read_text()
    f = g.facts(src); qs = g.questions(f, g.table(spec_md, "Transitions")); state = {"hook": src, "spec": spec_md}
    runs, missing = {}, {}
    for b in ("jev", "haiku", "luna"):
        try:
            runs[b] = run_request(name, "worktree", b, state, qs)
        except JevCheckError as exc:
            if exc.code != "KEY_MISSING":
                raise
            missing[b] = exc.code
    equal = all(r.input.equal for r in runs.values())
    rows = {k: {b: [r.answers[k]["choice"], round(r.answers[k]["confidence"], 2)] for b, r in runs.items()} for k in qs}
    agree = sum(len({v[0] for v in row.values()}) == 1 for row in rows.values())
    out[name] = {"missing": missing, "input": {b: dataclasses.asdict(r.input) for b, r in runs.items()},
                 "sessions": {b: r.session_id for b, r in runs.items()},
                 "equal": equal, "agreement": f"{agree}/{len(rows)}" if equal else "excluded: unequal input",
                 "rows": rows}
print(json.dumps(out, indent=1))
