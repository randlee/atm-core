"""Task templates must deliver variables verbatim.

sc-compose escapes every variable rendered from an ``.xml.j2`` template, the
same way it does for ``.html``. Task bodies are read by agents and never parsed
as XML, so an escaped variable hands the assignee ``&quot;`` and ``&lt;`` in
place of the JSON and shell text the caller supplied. Every repo-owned task
template therefore wraps its body in ``{% autoescape false %}`` and writes
pseudo-tags and heredocs as plain text. This test renders each template
through the real binary.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

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


def render(template: Path, values: dict[str, object]) -> str:
    with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
        json.dump(values, handle)
    completed = subprocess.run(
        ["sc-compose", "render", "--root", str(REPO_ROOT), "--file", str(template),
         "--var-file", handle.name],
        capture_output=True, text=True, check=False,
    )
    Path(handle.name).unlink()
    if completed.returncode != 0:
        raise AssertionError(f"sc-compose render failed: {completed.stderr.strip() or completed.stdout.strip()}")
    return completed.stdout


class TaskTemplateAutoescapeTests(unittest.TestCase):
    def test_templates_are_found(self) -> None:
        self.assertGreaterEqual(len(task_templates()), 11)

    def test_body_disables_autoescape_and_uses_plain_text(self) -> None:
        for template in task_templates():
            with self.subTest(template=template.name):
                _, body = split(template)
                self.assertTrue(body.startswith("{% autoescape false -%}\n"))
                self.assertTrue(body.endswith("\n{%- endautoescape %}\n"))
                self.assertIsNone(ENTITY.search(body))

    def test_variables_render_verbatim(self) -> None:
        for template in task_templates():
            with self.subTest(template=template.name):
                values = variables(template)
                rendered = render(template, values)
                self.assertIsNone(ENTITY.search(rendered))
                self.assertNotIn("autoescape", rendered)
                self.assertIn(PAYLOAD, rendered)


if __name__ == "__main__":
    unittest.main()
