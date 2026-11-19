from pathlib import Path

from qa_reviewers import qa_capable_reviewer, qa_capable_reviewers

TEMPLATES = Path(__file__).parents[2] / "templates"


def test_only_sprint_qa_contracts_qualify():
    for name in ("team-lead", "rust-developer", "dev-sanity", "plan-scope-reviewer", "../flaky-test-qa"):
        assert not qa_capable_reviewer(name, TEMPLATES)
    supported = {"arch-qa", "flaky-test-qa", "req-qa", "ruthless-boundary-qa", "schema-reviewer",
                 "rust-qa-agent", "rust-code-reviewer", "rust-best-practices-agent", "rust-service-hardening-agent"}
    assert set(qa_capable_reviewers(TEMPLATES)) == supported
    assert all(qa_capable_reviewer(name, TEMPLATES) for name in supported)
