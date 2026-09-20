"""Task templates must deliver variables verbatim through explicit boundaries.

sc-compose escapes every variable rendered from an ``.xml.j2`` template, the
same way it does for ``.html``. The QA template is strict XML and uses CDATA
for readable prompt text. Other templates retain their legacy plain-text
compatibility wrapper until they are migrated independently. This test renders
each template through the real binary and verifies its declared strategy.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess
import tempfile
import unittest
from xml.etree import ElementTree

REPO_ROOT = Path(__file__).resolve().parents[2]
TEMPLATE_DIRS = (
    ".claude/skills/codex-orchestration",
    ".claude/skills/graph-orchestration",
    ".claude/skills/plan-hardening",
)
PAYLOAD = '<tag> & "dq" \'sq\' {"k": "v"}'
ENTITY = re.compile(r"&(lt|gt|amp|quot|#\d+|#x[0-9a-fA-F]+);")


def task_templates() -> list[Path]:
    return sorted(
        path for directory in TEMPLATE_DIRS for path in (REPO_ROOT / directory).glob("*.xml.j2")
    )


def split(template: Path) -> tuple[str, str]:
    _, head, body = template.read_text(encoding="utf-8").split("---\n", 2)
    return head, body


def variables(template: Path) -> dict[str, object]:
    head, body = split(template)
    names = re.findall(r"^  - (\S+)$", head, re.MULTILINE)
    names += sorted(set(re.findall(r"{{\s*([a-z_]+)", body)) - set(names))
    values: dict[str, object] = {name: f"{name} {PAYLOAD}" for name in names}
    if "round_index" in values:
        values["round_index"] = 2
    return values


def render(template: Path, values: dict[str, object], *, strict: bool = False) -> str:
    with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
        json.dump(values, handle)
    command = [
        "sc-compose", "render", "--root", str(REPO_ROOT), "--file", str(template),
        "--var-file", handle.name,
    ]
    if strict:
        command.extend(["--strict", "--check-render", "--json"])
    completed = subprocess.run(
        command,
        capture_output=True, text=True, check=False,
    )
    Path(handle.name).unlink()
    if completed.returncode != 0:
        raise AssertionError(f"sc-compose render failed: {completed.stderr.strip() or completed.stdout.strip()}")
    if not strict:
        return completed.stdout
    envelope = json.loads(completed.stdout)
    errors = [
        diagnostic
        for diagnostic in envelope["diagnostics"]
        if diagnostic["severity"] == "error"
    ]
    if errors:
        raise AssertionError(f"strict validation errors: {errors}")
    return envelope["payload"]["body"]


class TaskTemplateAutoescapeTests(unittest.TestCase):
    def test_templates_are_found(self) -> None:
        self.assertGreaterEqual(len(task_templates()), 11)

    def test_templates_use_their_declared_escape_strategy(self) -> None:
        for template in task_templates():
            with self.subTest(template=template.name):
                _, body = split(template)
                if template.name == "qa-template.xml.j2":
                    self.assertNotIn("{% autoescape false", body)
                    self.assertNotIn("endautoescape", body)
                    self.assertNotIn("{% macro", body)
                    self.assertIn("| cdata_escape", body)
                    self.assertIn("<![CDATA[", body)
                else:
                    self.assertTrue(body.startswith("{% autoescape false -%}\n"))
                    self.assertTrue(body.endswith("\n{%- endautoescape %}\n"))
                self.assertIsNone(ENTITY.search(body))

    def test_variables_render_verbatim(self) -> None:
        for template in task_templates():
            with self.subTest(template=template.name):
                values = variables(template)
                rendered = render(
                    template,
                    values,
                    strict=template.name == "qa-template.xml.j2",
                )
                self.assertNotIn("autoescape", rendered)
                if template.name == "qa-template.xml.j2":
                    root = ElementTree.fromstring(rendered)
                    self.assertIn(PAYLOAD, root.attrib["id"])
                    self.assertIn(PAYLOAD, "".join(root.itertext()))
                    typed_values = {
                        **values,
                        "pr_number": 187,
                        "commits": [PAYLOAD, "def456"],
                        "review_targets": [PAYLOAD],
                        "changed_files": [PAYLOAD],
                        "triage_records": [PAYLOAD],
                        "references": [PAYLOAD],
                    }
                    typed_root = ElementTree.fromstring(
                        render(template, typed_values, strict=True)
                    )
                    self.assertEqual(
                        json.loads(typed_root.findtext("pr-number")),
                        typed_values["pr_number"],
                    )
                    for field, variable in (
                        ("commits", "commits"),
                        ("review-targets", "review_targets"),
                        ("changed-files", "changed_files"),
                        ("triage-records", "triage_records"),
                        ("references", "references"),
                    ):
                        self.assertEqual(
                            json.loads(typed_root.findtext(field)),
                            typed_values[variable],
                        )
                else:
                    self.assertIsNone(ENTITY.search(rendered))
                    self.assertIn(PAYLOAD, rendered)


if __name__ == "__main__":
    unittest.main()
