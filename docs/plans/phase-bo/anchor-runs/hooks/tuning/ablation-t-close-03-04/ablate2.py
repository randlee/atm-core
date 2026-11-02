# /// script
# dependencies = ["jev-check"]
# [tool.uv.sources]
# jev-check = { path = "/Users/randlee/github/atm-core-worktrees/plan/phase-bo-replan/docs/plans/phase-bo/design/packages/jev-check", editable = true }
# ///
import importlib.util, json
from pathlib import Path
from jev_check import check
spec = importlib.util.spec_from_file_location("g", "design/lite-code/atm-bd-lite-gate.py")
g = importlib.util.module_from_spec(spec); spec.loader.exec_module(g)
SPEC = Path("hooks/task-close.md").read_text()
U = Path("design/lite-code/gate-fixtures/close-unlisted-refusal.py").read_text()
HELD = "            if ledger.assignee and ledger.assignee != req.caller:\n                return HookReply.refused(Refusal(ErrorId.STACK_HELD, holder=ledger.assignee))\n"
CLEAN = U.replace(U.split('"""', 2)[1], "Lite task.close hook (hooks/task-close.md).\n").replace(HELD, "")
TL = SPEC[SPEC.index("Target layer (T-CLOSE-03/04)"):SPEC.index("## Facts")]
NOTHING = ("            if (target_id := bead.checked_bead or bead.closed_work_blocker()) is None:\n"
           "                return HookReply.refused(Refusal(ErrorId.QA_NOTHING_TO_REVIEW, bead=bead))\n")
assert U.count(NOTHING) == 2
NOTHING_LOOKUP = "            target_id = bead.checked_bead or bead.closed_work_blocker()\n"
variants = {"clean": (CLEAN, SPEC), "clean-spec-no-target-para": (CLEAN, SPEC.replace(TL, "")),
            "clean-no-nothing-refusal": (CLEAN.replace(NOTHING, NOTHING_LOOKUP), SPEC)}
out = {}
for name, (src, spec_md) in variants.items():
    f = g.facts(src); qs = g.questions(f, g.table(spec_md, "Transitions"))
    r = check({"hook": src, "spec": spec_md}, qs)
    out[name] = {k[-10:]: f'{a["choice"][0]}{a["confidence"]:.2f}' for k, a in r.answers.items() if k.startswith("implemented:")}
print(json.dumps(out))
