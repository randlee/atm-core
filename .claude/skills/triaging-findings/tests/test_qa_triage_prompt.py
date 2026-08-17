"""Contract checks for the qa-triage agent prompt's validation gate."""

from pathlib import Path


PROMPT = Path(__file__).resolve().parents[3] / "agents" / "qa-triage.md"


def test_qa_triage_prompt_validates_the_complete_phase_after_render() -> None:
    text = PROMPT.read_text(encoding="utf-8")

    assert "validate-findings.py" in text
    assert "--findings-dir \"$triage_root/$phase_id/findings\"" in text
    assert "--structure \"$structure_path\"" in text
    assert "--events \"$events_path\"" in text
    assert "--json" in text
    assert 'kind == "validation:pass"' in text
    assert "validation:fail" in text
    assert "error" in text


def test_qa_triage_prompt_does_not_treat_validation_fail_as_success() -> None:
    text = PROMPT.read_text(encoding="utf-8")

    gate = text[
        text.index("12. Validate the rendered Turtle") : text.index(
            "13. Return enough information"
        )
    ]
    assert "blocks this agent from reporting success" in gate
    assert "only `validation:pass`" in gate
