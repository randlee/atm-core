from pathlib import Path
import subprocess


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


def test_oversized_jev_request_is_announced_and_never_starts_an_llm_check():
    """#365: an oversized Jev request is announced with its size and never becomes a new LLM or haiku check."""
    dev = " ".join((AGENTS / "dev-sanity.md").read_text().split())
    jev = " ".join((AGENTS / "sc-sanity-jev.md").read_text().split())
    rule = "is never a reason to run an LLM or haiku check in the Jev slot"
    assert rule in dev and rule in jev
    assert "jev_client.py --announce --phase <phase> --error" in dev
    assert "the message names the request size" in dev
    assert "never a new check" in dev
    assert "`SANITY.JEV_REQUEST_TOO_LARGE` it first announces the verbatim error" in jev


def test_qa_reviewers_leave_upstream_owned_installed_content_to_the_lead():
    """#5: upstream-owned installed .claude/ content is out of QA reviewer scope."""
    role = " ".join((AGENTS.parent / "skills/atm-bd-orchestration/roles/quality-mgr.md").read_text().split())
    assert "Upstream-owned installed `.claude/` skill, agent and template content is out of QA reviewer scope" in role
    assert "the lead accepts such changes by diff review and the package's template tests" in role
    # The exclusion is bounded: the repository's own change and the checked bead stay in QA scope.
    for kept in ("the checked bead is authoritative", "add any reviewer the repository QA policy (`policy_path`) requires for the change"):
        assert kept in role
    assert "out of QA reviewer scope" not in role.replace(
        "Upstream-owned installed `.claude/` skill, agent and template content is out of QA reviewer scope", "")


def test_qa_template_names_the_installed_stack_view_skill_only():
    """#357: the QA template names `sc-gh-stack-view`, with no `gh-stack-view` fallback."""
    template = (AGENTS.parent / "skills/atm-bd-orchestration/templates/qa-template.xml.j2").read_text()
    assert "the `sc-gh-stack-view` skill" in template
    assert "`gh-stack-view`" not in template
    examples = AGENTS.parent / "skills/atm-bd-orchestration/examples"
    for example in ("qa-template-vars.json", "qa-template-fix-round-vars.json"):
        rendered = subprocess.run(["sc-compose", "render", "--strict", "--file", str(AGENTS.parent / "skills/atm-bd-orchestration/templates/qa-template.xml.j2"),
                                   "--var-file", str(examples / example)], capture_output=True, text=True, check=True).stdout
        assert ("the `sc-gh-stack-view` skill" in rendered) == (example == "qa-template-vars.json")  # step h needs a PR
        assert "`gh-stack-view`" not in rendered
