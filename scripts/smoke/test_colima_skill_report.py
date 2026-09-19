"""Unit tests for the colima skill integration-step converter."""
from __future__ import annotations

import json
from pathlib import Path
import tempfile
from unittest import mock
import unittest

from scripts.smoke import colima_skill_report as MODULE

REPORT = """ATM TEST REPORT
skill: atm-smoke
fixture: hermes-testbed
agent: tester@testbed  tools: cli
atm: client 1.5.9 daemon 1.5.9
result: PASS   (2/2 steps)
steps:
  0 PASS Self round trip — message_id: X, count: 1→0
  1 FAIL Ack — exit code: 1
  2 PASS Doctor passes
elapsed: 56s
"""
RESULT = """PASS  skills reported 7/7, PASS 7/7 (7 report messages)
atm:    1.5.9 (prerelease/v1.5.9 @ 0c2d38368, prerelease-archive run 1, ci run 2)
herdr transport: socket ({'kind': 'ready'}) (atm doctor --json .herdr.endpoints[].transport)
"""


class ParseTests(unittest.TestCase):
    def test_steps_become_cases_named_by_skill_and_agent(self) -> None:
        cases = MODULE.parse_report(REPORT)
        self.assertEqual(
            [case["name"] for case in cases],
            [
                "atm-smoke / tester: Self round trip",
                "atm-smoke / tester: Ack",
                "atm-smoke / tester: Doctor passes",
            ],
        )
        self.assertEqual([case["status"] for case in cases], ["PASS", "FAIL", "PASS"])

    def test_header_carries_version_and_transport(self) -> None:
        doctor, advertised, transport, testbed = MODULE.header_cases(RESULT, MODULE.parse_report(REPORT))
        self.assertEqual((doctor["status"], doctor["detail"]), ("PASS", "ATM 1.5.9"))
        self.assertEqual(advertised["name"], "advertised host")
        self.assertEqual((transport["status"], transport["detail"]), ("PASS", "socket ({'kind': 'ready'})"))
        self.assertEqual(testbed["name"], "testbed ref")

    def test_header_doctor_fails_without_a_passing_doctor_step(self) -> None:
        doctor, _, transport, _ = MODULE.header_cases(
            RESULT.replace("socket (", "cli ("),
            MODULE.parse_report(REPORT.replace("2 PASS Doctor", "2 FAIL Doctor")),
        )
        self.assertEqual(doctor["status"], "FAIL")
        self.assertEqual(transport["status"], "FAIL")


class StepTests(unittest.TestCase):
    def test_run_step_writes_payload_and_copies_raw_sidecars(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            run_dir = Path(tempdir) / "20260908T162312Z"
            run_dir.mkdir()
            (run_dir / "result.txt").write_text(RESULT, encoding="utf-8")
            (run_dir / "report-1.txt").write_text(REPORT, encoding="utf-8")
            (run_dir / "herdr-doctor.json").write_text("{}", encoding="utf-8")
            step_dir = Path(tempdir) / "step"
            with mock.patch.object(MODULE, "_source_revision", return_value="b" * 40):
                payload = MODULE.run_step(run_dir, step_dir)
            self.assertEqual(json.loads((step_dir / "step.json").read_text()), payload)
            self.assertEqual(payload["source_revision"], "b" * 40)
            self.assertEqual(payload["run_id"], run_dir.name)
            self.assertIn("testbed ref", [item["name"] for item in payload["cases"]])
            self.assertEqual(
                {path.name for path in step_dir.iterdir()},
                {"step.json", "result.txt", "report-1.txt", "herdr-doctor.json"},
            )

    def test_documented_steps_match_the_runner_case_order(self) -> None:
        root = Path(__file__).resolve().parents[2]
        header_names = [item["name"] for item in MODULE.header_cases(RESULT, MODULE.parse_report(REPORT))]
        source = (root / "docs/procedures/colima-hermes-skills.md").read_text(encoding="utf-8")
        sections = source.split("## Steps\n")[1:]
        self.assertEqual(len(sections), 2)
        for section in sections:
            table = section.split("\n## ", 1)[0]
            documented = [
                line.split("`", 2)[1]
                for line in table.splitlines()
                if line.startswith("| ") and "Execute `" in line
            ]
            self.assertEqual(len(documented), 49)
            self.assertEqual(documented[: len(header_names)], header_names)


if __name__ == "__main__":
    unittest.main()
