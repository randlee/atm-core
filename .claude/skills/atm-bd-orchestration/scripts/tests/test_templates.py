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
        import json, sys
        sys.path.insert(0, str(ROOT.parents[1] / "atm-beads" / "scripts"))
        from plan_contract import SEVERITY_PRIORITY
        self.assertIn(json.dumps(SEVERITY_PRIORITY).replace(", ", ", "), text)  # the literal mirrors plan_contract; Jinja cannot import it
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
        templates = sorted((ROOT / "templates").glob("*.j2"))
        self.assertTrue(templates, "templates directory must not be empty")
        for template_path in templates:
            fixture_name = template_path.name.removesuffix(".j2").rsplit(".", 1)[0] + "-vars.json"
            variables = examples / fixture_name
            with self.subTest(template=template_path.name):
                self.assertTrue(variables.is_file(), f"missing strict-render fixture: {fixture_name}")
                result = subprocess.run([
                    "sc-compose", "render", "--file", str(template_path),
                    "--var-file", str(variables), "--strict"], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
