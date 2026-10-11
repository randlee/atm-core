from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_template_frontmatter import find_violations


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


if __name__ == "__main__":
    unittest.main()
