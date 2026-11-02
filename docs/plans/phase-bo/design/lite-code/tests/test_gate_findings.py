import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location("gate", Path(__file__).parents[1] / "atm-bd-lite-gate.py")
assert spec is not None and spec.loader is not None
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


def test_one_no_row_and_one_unlisted_branch_map_to_exactly_two_findings() -> None:
    yes, no = {"choice": "yes"}, {"choice": "no"}
    answers = {"implemented:T-CLOSE-01": yes, "implemented:T-CLOSE-02": no, "listed:L28": yes, "listed:L38": no}
    facts = {"arms": [("Transition.GIVE_BACK", 28)],
             "replies": [("HookReply.refused(Refusal(ErrorId.STACK_HELD))", 38)]}
    assert gate.findings(answers, facts) == [
        "implement T-CLOSE-02",
        "add a row for L38 `HookReply.refused(Refusal(ErrorId.STACK_HELD))`, or remove the branch"]
