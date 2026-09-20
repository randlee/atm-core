"""QA assignment templates must render carry-forward findings as JSON arrays."""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
ASSIGNMENT_DIRS = (
    ".claude/assets/sc-rust/quality-mgr/templates",
    ".claude/skills/codex-orchestration",
)
POPULATED = [
    {"id": "REQ-1", "note": 'say "hi" <tag> & a\\b'},
    {"id": "REQ-2"},
]


def assignment_templates() -> list[Path]:
    return sorted(
        template
        for directory in ASSIGNMENT_DIRS
        for template in (REPO_ROOT / directory).glob("*-assignment.json.j2")
    )


def required_values(template: Path) -> dict[str, object]:
    _, frontmatter, body = template.read_text(encoding="utf-8").split("---\n", 2)
    required_block = frontmatter.split("required_variables:\n", 1)[1]
    names: list[str] = []
    for line in required_block.splitlines():
        if not line.startswith("  - "):
            break
        names.append(line[4:].strip())
    list_variables = set(re.findall(r"{% for \w+ in (\w+) %}", body))
    return {name: [name] if name in list_variables else name for name in names}


def render(template: Path, values: dict[str, object]) -> dict[str, object]:
    with tempfile.TemporaryDirectory() as scratch:
        variables = Path(scratch) / "vars.json"
        variables.write_text(json.dumps(values), encoding="utf-8")
        result = subprocess.run(
            [
                "sc-compose",
                "render",
                "--root",
                str(REPO_ROOT),
                "--file",
                str(template.relative_to(REPO_ROOT)),
                "--var-file",
                str(variables),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
    if result.returncode != 0:
        raise AssertionError(f"{template.name}: exit {result.returncode}\n{result.stderr}")
    return json.loads(result.stdout)


class QaTemplateJsonListVarsTests(unittest.TestCase):
    def test_carry_forward_findings_remain_arrays(self) -> None:
        templates = assignment_templates()
        self.assertEqual(len(templates), 7)
        for template in templates:
            for supplied, expected, locked in (
                ("[]", [], False),
                (json.dumps(POPULATED), POPULATED, True),
            ):
                with self.subTest(template=template.name, populated=locked):
                    values = required_values(template)
                    values["carry_forward_findings_json"] = supplied
                    rendered = render(template, values)
                    self.assertEqual(rendered["carry_forward_findings"], expected)
                    if "findings_scope_locked" in rendered:
                        self.assertIs(rendered["findings_scope_locked"], locked)


if __name__ == "__main__":
    unittest.main()
