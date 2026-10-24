from pathlib import Path


AGENTS = Path(__file__).resolve().parents[4] / "agents"


def test_sanity_prompts_close_before_ordered_stack_handoff():
    dev = (AGENTS / "dev-sanity.md").read_text()
    stack = (AGENTS / "sanity-stack.md").read_text()
    normalized_dev = " ".join(dev.split())
    for phrase in ("never edit generated vars", "Only after successful PASS close", "One handoff at a time",
                   "leave the sanity task closed", "no stack outcome increments sanity_round",
                   "up to three times", "llm_elapsed_s", "API_FAILURE", "neither child looks up beads"):
        assert phrase.lower() in normalized_dev.lower()
    for phrase in ("exactly one next-top layer", "Return the first", "Never reopen the sanity task"):
        assert phrase in stack
    normalized_stack = " ".join(stack.split())
    assert "do not wait for a person or retry a repair" in normalized_stack
    assert normalized_dev.index("Run atm task close") < normalized_dev.index("Only after successful PASS close")
    assert "keep the sanity task active" not in normalized_dev
    for prompt in (dev, stack):
        for forbidden in ("class bead", "outage bead", "hold on re-dispatch", "citation", "checker-defect", "sanity-run-table"):
            assert forbidden not in prompt
