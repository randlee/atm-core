from __future__ import annotations

from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import patch


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_template_frontmatter import find_violations, template_violations, tracked_templates


DECLARED = "---\nname: t\nversion: 1.0.0\ndescription: t\nrequired_variables:\n  - who\n  - report.status\n---\n"


class TemplateFrontmatterTests(unittest.TestCase):
    def check(self, text: str) -> list[str]:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "t.j2").write_text(text, encoding="utf-8")
            return find_violations(root, ["t.j2"])

    def test_accepts_template_declaring_every_variable(self) -> None:
        self.assertEqual(self.check(DECLARED + "{{ who }} {{ report.status }}\n"), [])

    def test_rejects_template_without_front_matter(self) -> None:
        self.assertIn("no front matter", "\n".join(self.check("{{ who }}\n")))

    def test_rejects_variable_missing_from_front_matter(self) -> None:
        self.assertIn("`when`", "\n".join(self.check(DECLARED + "{{ who }} {{ when }}\n")))

    def test_rejects_undeclared_dotted_field(self) -> None:
        self.assertIn("`report.summary.pass`", "\n".join(self.check(DECLARED + "{{ report.summary.pass }}\n")))

    def test_ignores_the_jinja_keywords_misreported_by_sc_compose(self) -> None:
        text = DECLARED + "{% autoescape false %}{{ who }}{% if who is defined %}x{% endif %}{% endautoescape %}\n"
        self.assertEqual(self.check(text), [])

    def test_rejects_unrecognized_strict_validation_failure_and_preserves_output(self) -> None:
        completed = subprocess.CompletedProcess(
            args=[], returncode=2, stdout="ERR_PARSE_INVALID: invalid template syntax\n", stderr=""
        )
        with tempfile.TemporaryDirectory() as temporary, patch(
            "check_template_frontmatter.subprocess.run", return_value=completed
        ):
            violations = template_violations(Path(temporary), "broken.j2")

        self.assertEqual(len(violations), 1)
        self.assertIn("exit 2", violations[0])
        self.assertIn("ERR_PARSE_INVALID: invalid template syntax", violations[0])

    def test_allows_only_the_sc_compose_784_keyword_diagnostics(self) -> None:
        completed = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout=(
                "ERR_VAL_UNDECLARED_TOKEN: undeclared referenced token: autoescape\n"
                "ERR_VAL_UNDECLARED_TOKEN: undeclared referenced token: endautoescape\n"
            ),
            stderr="",
        )
        with tempfile.TemporaryDirectory() as temporary, patch(
            "check_template_frontmatter.subprocess.run", return_value=completed
        ):
            violations = template_violations(Path(temporary), "keywords.j2")

        self.assertEqual(violations, [])

    def test_tracked_templates_includes_installed_orchestration_templates(self) -> None:
        listed = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=(
                "docs/example.j2\n"
                ".claude/skills/atm-bd-orchestration/templates/task-assignment.xml.j2\n"
            ),
            stderr="",
        )
        with patch("check_template_frontmatter.subprocess.run", return_value=listed):
            templates = tracked_templates(Path("repo"))

        self.assertEqual(len(templates), 2)
        self.assertTrue(any("atm-bd-orchestration" in template for template in templates))


if __name__ == "__main__":
    unittest.main()
