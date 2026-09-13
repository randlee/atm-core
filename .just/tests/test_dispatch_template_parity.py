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
# Every agent-facing instruction file is swept; docs/plans/ is excluded because
# sprint records quote the retired wording as finding text (BB2-017).
SEQUENCE_DOC_ROOTS = ("AGENTS.md", "CLAUDE.md", "docs", ".claude")
SEQUENCE_DOC_SUFFIXES = (".md", ".j2", ".txt")
SEQUENCE_DOC_EXCLUDED = ("docs/plans/",)
STALE_SEQUENCE = re.compile(
    r"\back(?:\s*,\s*|\s+then\s+)work(?:\s*,\s+|\s+then\s+)(?:task\s+)?close\b"
    r"|completion ack(nowledgement)? by (the )?receiver"
    r"|receiver acknowledg"
    r"|acknowledges completion"
    r"|ack -> work -> completion",
    re.IGNORECASE,
)
ELEMENT = re.compile(r"<stack-discipline>(.*?)</stack-discipline>", re.DOTALL)


def sequence_docs() -> list[str]:
    found: list[str] = []
    for root in SEQUENCE_DOC_ROOTS:
        base = ROOT / root
        paths = [base] if base.is_file() else sorted(base.rglob("*"))
        for path in paths:
            if not path.is_file() or path.suffix not in SEQUENCE_DOC_SUFFIXES:
                continue
            rel = path.relative_to(ROOT).as_posix()
            if rel.startswith(SEQUENCE_DOC_EXCLUDED):
                continue
            found.append(rel)
    return found


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
        offenders = []
        for rel in sequence_docs():
            if STALE_SEQUENCE.search((ROOT / rel).read_text(errors="replace")):
                offenders.append(rel)
        self.assertEqual(offenders, [], "retired receiver completion-ACK step is back in these files")

    def test_sequence_sweep_covers_the_protocol_doc(self) -> None:
        docs = set(sequence_docs())
        for rel in ("docs/team-protocol.md", "AGENTS.md", "CLAUDE.md",
                    ".claude/skills/quality-management-gh/SKILL.md"):
            self.assertIn(rel, docs)


if __name__ == "__main__":
    unittest.main()
