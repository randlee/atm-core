"""Mechanical guard for text that must stay identical across dispatch files.

The ATM template renderer refuses `include`, so the `<stack-discipline>`
element cannot come from one shared partial; this test is the single source
of truth's enforcement instead (BB2-006). It also keeps the retired
receiver-side completion ACK from returning to any skill or protocol doc
(BB2-005, BB2-007).
"""

from __future__ import annotations

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
STACK_DISCIPLINE_TEMPLATES = (
    ".claude/skills/codex-orchestration/dev-template.xml.j2",
    ".claude/skills/codex-orchestration/fix-assignment.xml.j2",
    ".claude/skills/graph-orchestration/dev-task.xml.j2",
    ".claude/skills/graph-orchestration/dev-fix.xml.j2",
)
SEQUENCE_DOCS = (
    "docs/team-protocol.md",
    ".claude/skills/codex-orchestration/SKILL.md",
    ".claude/skills/graph-orchestration/SKILL.md",
    ".claude/skills/triaging-findings/SKILL.md",
)
ELEMENT = re.compile(r"<stack-discipline>(.*?)</stack-discipline>", re.DOTALL)


class DispatchTemplateParity(unittest.TestCase):
    def test_stack_discipline_element_is_identical_across_dev_templates(self) -> None:
        bodies = {}
        for rel in STACK_DISCIPLINE_TEMPLATES:
            found = ELEMENT.findall((ROOT / rel).read_text())
            self.assertEqual(len(found), 1, f"{rel}: expected exactly one <stack-discipline> element")
            bodies[rel] = found[0]
        reference = bodies[STACK_DISCIPLINE_TEMPLATES[0]]
        for rel, body in bodies.items():
            self.assertEqual(body, reference, f"{rel}: <stack-discipline> text drifted from {STACK_DISCIPLINE_TEMPLATES[0]}")

    def test_no_receiver_completion_ack_survives(self) -> None:
        stale = re.compile(r"completion ack by (the )?receiver|acknowledges completion", re.IGNORECASE)
        for rel in SEQUENCE_DOCS:
            self.assertIsNone(stale.search((ROOT / rel).read_text()), f"{rel}: retired receiver completion-ACK step is back")


if __name__ == "__main__":
    unittest.main()
