"""Contract checks for the canonical benchmark operator procedure."""
from __future__ import annotations

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
SKILL = ROOT / ".claude/skills/benchmark-run/SKILL.md"


class BenchmarkRunSkillTests(unittest.TestCase):
    def setUp(self) -> None:
        self.text = SKILL.read_text(encoding="utf-8")

    def test_skill_contains_complete_operator_sequence(self) -> None:
        for heading in (
            "## 1. Preflight",
            "## 2. Run",
            "## 3. Review",
            "## 4. Publish",
            "## 5. INCOMPLETE campaigns",
            "## 6. Failure classification and rerun policy",
            "## 7. Windows appendix",
        ):
            self.assertIn(heading, self.text)
        for command in (
            "just benchmark-bootstrap",
            "ATM_CAPACITY_HOST_LABEL=rand-m5 just benchmark",
            "just benchmark-show",
            "just benchmark-publish",
            "just reports-index --check",
        ):
            self.assertIn(command, self.text)

    def test_skill_preserves_complete_os_matrices_and_safe_failure_rules(self) -> None:
        self.assertIn("`sqlite`, `uds`,", self.text)
        self.assertIn("`tcp`, and\n`tcp-tls`", self.text)
        self.assertIn("(`sqlite`, `tcp`, `tcp-tls`)", self.text)
        self.assertIn("never rewrite or remove the incomplete one ad hoc", self.text)
        self.assertIn("Never run a benchmark against a developer account", self.text)
        self.assertIn("two selector symlinks", self.text)


if __name__ == "__main__":
    unittest.main()
