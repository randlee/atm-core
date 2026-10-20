from pathlib import Path


AGENTS = Path(__file__).resolve().parents[4] / "agents"


def test_sanity_prompts_describe_ordered_linking_and_failures():
    dev = (AGENTS / "dev-sanity.md").read_text()
    stack = (AGENTS / "sanity-stack.md").read_text()
    for phrase in ("more than one open sanity task", "one spawn at a time", "never two in flight",
                   "only then run `atm-task-close.py", "sanity_round` unchanged", "keep the sanity task active",
                   "send the holder one plain", "re-spawn after", "report the evidence to the lead",
                   "one Jev client call", "up to three times", "llm_elapsed_s", "API_FAILURE"):
        assert phrase in dev
    for phrase in ("exactly one next-top layer", "Never rewrite a layer with children", "Make no ATM writes"):
        assert phrase in stack
    normalized_stack = " ".join(stack.split())
    assert "never refuse the task" in normalized_stack
    assert "do not send the dev a FAIL" in normalized_stack
    assert "do not escalate to a person" in normalized_stack
    normalized_dev = " ".join(dev.split())
    assert "or when the inline result is `stack_failed`, spawn `sanity-stack`" in normalized_dev
    lock_path = normalized_dev.split("On `lock_timeout`,", 1)[1].split("On subagent `stack_failed`", 1)[0]
    assert "keep the sanity task active" in lock_path
    assert "Never refuse the task, send the dev a FAIL, or escalate to a person for a lock." in lock_path
    for prompt in (dev, stack):
        for forbidden in ("class bead", "outage bead", "hold on re-dispatch", "citation", "checker-defect", "sanity-run-table"):
            assert forbidden not in prompt
