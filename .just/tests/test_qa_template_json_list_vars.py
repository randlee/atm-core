"""List-valued template variables must render as JSON arrays.

Callers pass ``carry_forward_findings_json`` and ``blocking_ids_json`` as JSON
text. sc-compose JSON-encodes string variables in ``.json.j2`` templates, so
without the ``safe`` filter the receiving agent gets ``"[]"`` (a string) where
its contract expects an array. Markdown report templates are not encoded and
need no filter; they are rendered here as a guard. This test renders each
template through the real binary with the default and with a populated value,
and also renders the plan-hardening example vars files against the templates
they document.
"""

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
REPORTS = (
    ".claude/skills/quality-management-gh/findings-report.md.j2",
    ".claude/skills/quality-management-gh/quality-report.md.j2",
)
EXAMPLES = (
    ("plan-review-notice.xml.j2", "plan-review-notice-vars.example.json"),
    ("plan-critical-review.xml.j2", "plan-critical-review-vars.example.json"),
)
POPULATED = [{"id": "REQ-1", "note": 'say "hi" <t> & a\\b'}, {"id": "REQ-2"}]


def assignment_templates() -> list[Path]:
    return sorted(
        path
        for directory in ASSIGNMENT_DIRS
        for path in (REPO_ROOT / directory).glob("*-assignment.json.j2")
    )


def required(template: Path) -> dict[str, object]:
    _, head, body = template.read_text(encoding="utf-8").split("---\n", 2)
    block = head.split("required_variables:\n", 1)[1]
    names = []
    for line in block.splitlines():
        if not line.startswith("  - "):
            break
        names.append(line[4:].strip())
    lists = set(re.findall(r"{% for \w+ in (\w+) %}", body))
    return {name: [name] if name in lists else name for name in names}


def render(template: Path, values: dict[str, object]) -> str:
    with tempfile.TemporaryDirectory() as scratch:
        var_file = Path(scratch) / "vars.json"
        var_file.write_text(json.dumps(values), encoding="utf-8")
        result = subprocess.run(
            [
                "sc-compose",
                "render",
                "--root",
                str(REPO_ROOT),
                "--file",
                str(template.relative_to(REPO_ROOT)),
                "--var-file",
                str(var_file),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
    if result.returncode != 0:
        raise AssertionError(f"{template.name}: exit {result.returncode}\n{result.stderr}")
    return result.stdout


class QaTemplateJsonListVarsTests(unittest.TestCase):
    def test_assignment_templates_render_carry_forward_as_an_array(self) -> None:
        templates = assignment_templates()
        self.assertGreaterEqual(len(templates), 7)
        for template in templates:
            values = required(template)
            for supplied, expected, locked in (
                (None, [], False),
                (json.dumps(POPULATED), POPULATED, True),
            ):
                with self.subTest(template=template.name, populated=locked):
                    if supplied is not None:
                        values["carry_forward_findings_json"] = supplied
                    rendered = json.loads(render(template, values))
                    self.assertEqual(rendered["carry_forward_findings"], expected)
                    if "findings_scope_locked" in rendered:
                        self.assertIs(rendered["findings_scope_locked"], locked)

    def test_report_templates_render_blocking_ids_as_an_array(self) -> None:
        for relative in REPORTS:
            template = REPO_ROOT / relative
            for ids in ([], ["QA-001", "QA-002"]):
                with self.subTest(template=template.name, ids=ids):
                    values = required(template)
                    values.update(
                        pr_number=1,
                        deliverables_complete=1,
                        deliverables_total=1,
                        deliverables_percent=100,
                        findings_blocking=len(ids),
                        findings_important=0,
                        findings_minor=0,
                        blocking_ids_json=json.dumps(ids),
                    )
                    rendered = render(template, values)
                    fenced = re.search(r"```json\n(.*?)\n```", rendered, re.DOTALL)
                    self.assertIsNotNone(fenced)
                    self.assertEqual(json.loads(fenced.group(1))["blocking_ids"], ids)

    def test_plan_hardening_example_vars_render_their_templates(self) -> None:
        base = REPO_ROOT / ".claude/skills/plan-hardening"
        for template_name, example_name in EXAMPLES:
            with self.subTest(template=template_name):
                values = json.loads((base / "examples" / example_name).read_text(encoding="utf-8"))
                self.assertIn(str(values["phase"]), render(base / template_name, values))


if __name__ == "__main__":
    unittest.main()
