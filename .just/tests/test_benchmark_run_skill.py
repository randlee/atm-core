"""Contract checks for the canonical benchmark operator procedure."""
from __future__ import annotations

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
SKILL = ROOT / ".claude/skills/benchmark-run/SKILL.md"
SUPERSEDED_DOCS = (
    "docs/plans/phase-ai/sprint-ai-40-local-transport-benchmark.md",
    "docs/plans/phase-ai/sprint-ai-49-benchmark-report.md",
    "docs/plans/phase-ai/sprint-ai-52-windows-transport-benchmark-confirmation.md",
    "docs/plans/phase-al/AL9-benchmark-gate.md",
    "docs/plans/phase-ao2/sprint-AO2-5-4-mandatory-benchmark-snapshot-restore.md",
    "docs/plans/phase-ao2/sprint-AO2-7-m5-tcp-benchmark-parity.md",
    "docs/plans/phase-ao2/sprint-AO2-8-windows-tcp-benchmark-parity.md",
)


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

    def test_docs_point_to_the_single_current_procedure(self) -> None:
        for relative in SUPERSEDED_DOCS:
            self.assertIn("benchmark-run/SKILL.md", (ROOT / relative).read_text(encoding="utf-8"))
        cross_platform = (ROOT / "docs/cross-platform-guidelines.md").read_text(encoding="utf-8")
        self.assertIn("Windows Benchmark Execution", cross_platform)
        self.assertIn("windows-x64-01", cross_platform)
        self.assertIn("benchmark-run/SKILL.md", (ROOT / "docs/plans/phase-ao2/README.md").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
