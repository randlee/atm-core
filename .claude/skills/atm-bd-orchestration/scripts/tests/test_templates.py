from __future__ import annotations

from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).parents[2]


class TemplateContractTests(unittest.TestCase):
    def test_dev_template_requires_pr_target_not_obsolete_top(self):
        text = (ROOT / "templates/dev-template.xml.j2").read_text()
        self.assertIn("- pr_target", text)
        self.assertNotIn("- top", text)
        self.assertNotIn("pushed" + " top", text)

    def test_finding_requires_difficulty_and_priority_map_is_current(self):
        text = (ROOT / "templates/finding-bead.json.j2").read_text()
        self.assertIn("- difficulty", text)
        self.assertIn('"important": 2', text)
        self.assertIn('"difficulty"', text)
        self.assertIn("## Deliverables\\n1.", text)

    def test_sanity_assignment_has_pr_and_exact_checks(self):
        text = (ROOT / "templates/dev-sanity-template.xml.j2").read_text()
        self.assertIn("- pr_number", text)
        self.assertIn("- pr_url", text)
        self.assertIn("gh pr view", text)
        self.assertIn("SANITY.ZERO_DELTA", text)

    def test_workflow_issue_template_exists(self):
        self.assertTrue((ROOT / "templates/workflow-issue-bead.json.j2").exists())

    def test_assignment_examples_render_strictly(self):
        examples = ROOT / "examples"
        mapping = {
            "dev-template-vars.json": "dev-template.xml.j2",
            "dev-fix-vars.json": "dev-fix.xml.j2",
            "dev-sanity-template-vars.json": "dev-sanity-template.xml.j2",
            "finding-bead-vars.json": "finding-bead.json.j2",
            "fix-assignment-vars.json": "fix-assignment.xml.j2",
            "qa-template-vars.json": "qa-template.xml.j2",
            "review-template-vars.json": "review-template.xml.j2",
        }
        for variables, template in mapping.items():
            with self.subTest(template=template):
                result = subprocess.run([
                    "sc-compose", "render", "--file", str(ROOT / "templates" / template),
                    "--var-file", str(examples / variables), "--strict"], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
