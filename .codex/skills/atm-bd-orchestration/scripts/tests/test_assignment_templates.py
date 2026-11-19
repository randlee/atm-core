from pathlib import Path

from rendering import render

ROOT = Path(__file__).parents[2]


def test_assignments_use_assigned_stack_and_sanity_notes():
    for name in ("dev-template", "fix-assignment", "qa-template"):
        result = render(ROOT / f"templates/{name}.xml.j2", ROOT / f"examples/{name}-vars.json")
        assert result.returncode == 0, result.stderr
        assert "sc-gh-stack-view" not in result.stdout
        if name != "qa-template":
            assert "sanity FAIL round <n> at <sha>:" in result.stdout
