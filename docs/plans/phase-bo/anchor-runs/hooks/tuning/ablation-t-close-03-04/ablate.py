# /// script
# dependencies = ["jev-check"]
# [tool.uv.sources]
# jev-check = { path = "/Users/randlee/github/atm-core-worktrees/plan/phase-bo-replan/docs/plans/phase-bo/design/packages/jev-check", editable = true }
# ///
"""Jev ablation for T-CLOSE-03/04: same spec and question set; one change per variant. Run from docs/plans/phase-bo."""
import dataclasses, importlib.util, json, sys
from pathlib import Path
from jev_check import check
from jev_check.checks import input_record

spec = importlib.util.spec_from_file_location("g", "design/lite-code/atm-bd-lite-gate.py")
g = importlib.util.module_from_spec(spec); spec.loader.exec_module(g)
spec_md = Path("hooks/task-close.md").read_text()
U = Path("design/lite-code/gate-fixtures/close-unlisted-refusal.py").read_text()
E = Path("design/lite-code/gate-fixtures/close-extra-write.py").read_text()
doc_u, doc_e = U.split('"""', 2)[1], E.split('"""', 2)[1]
NEUTRAL = "Lite task.close hook (hooks/task-close.md).\n"
HELD = "            if ledger.assignee and ledger.assignee != req.caller:\n                return HookReply.refused(Refusal(ErrorId.STACK_HELD, holder=ledger.assignee))\n"
assert HELD in U
variants = {
    "U": U,
    "U-neutral-doc": U.replace(doc_u, NEUTRAL),
    "U-no-held": U.replace(HELD, ""),
    "U-neutral-doc-no-held": U.replace(doc_u, NEUTRAL).replace(HELD, ""),
    "E": E,
    "E-neutral-doc": E.replace(doc_e, NEUTRAL),
}
out = {}
for name in sys.argv[1:] or variants:
    src = variants[name]
    f = g.facts(src); qs = g.questions(f, g.table(spec_md, "Transitions")); state = {"hook": src, "spec": spec_md}
    r = check(state, qs)
    out[name] = {k: [a["choice"], round(a["confidence"], 2)] for k, a in r.answers.items() if k.startswith("implemented:")}
    out[name]["input"] = dataclasses.asdict(input_record(state, qs, r.input_tokens, None, 0))["input_tokens"]
    Path(f"abl/{name}.py").parent.mkdir(exist_ok=True)
print(json.dumps(out, indent=1))
