"""Unit tests for the colima skill-run evidence renderer."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from unittest import mock
import unittest


def load_module():
    path = Path(__file__).with_name("colima_skill_report.py")
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("colima_skill_report", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


MODULE = load_module()

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
    def test_steps_become_cases_named_by_skill_and_agent(self):
        cases = MODULE.parse_report(REPORT)
        self.assertEqual([c["name"] for c in cases], [
            "atm-smoke / tester: Self round trip", "atm-smoke / tester: Ack", "atm-smoke / tester: Doctor passes",
        ])
        self.assertEqual([c["status"] for c in cases], ["PASS", "FAIL", "PASS"])
        self.assertEqual(cases[0]["detail"], "message_id: X, count: 1→0")
        self.assertEqual(cases[2]["detail"], "Doctor passes")
        self.assertTrue(all(c["origin"] == c["destination"] == MODULE.HOST for c in cases))

    def test_header_carries_version_and_transport(self):
        doctor, advertised, transport, testbed = MODULE.header_cases(RESULT, MODULE.parse_report(REPORT))
        self.assertEqual((doctor["status"], doctor["detail"]), ("PASS", "ATM 1.5.9"))
        self.assertEqual(advertised["name"], "advertised host")
        self.assertEqual((transport["status"], transport["detail"]), ("PASS", "socket ({'kind': 'ready'})"))
        self.assertEqual(testbed["name"], "testbed ref")

    def test_header_doctor_fails_without_a_passing_doctor_step(self):
        doctor, _, transport, _ = MODULE.header_cases(
            RESULT.replace("socket (", "cli ("), MODULE.parse_report(REPORT.replace("2 PASS Doctor", "2 FAIL Doctor")))
        self.assertEqual(doctor["status"], "FAIL")
        self.assertEqual(transport["status"], "FAIL")


class RenderTests(unittest.TestCase):
    def test_payload_and_envelope_carry_source_revision_and_testbed_ref_case(self):
        self.assertIn('"source_revision": source_revision', Path(MODULE.__file__).read_text(encoding="utf-8"))
        self.assertIn('case("testbed ref"', Path(MODULE.__file__).read_text(encoding="utf-8"))

    def test_render_writes_the_smoke_evidence_set(self):
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp) / "20260908T162312Z"
            run_dir.mkdir()
            (run_dir / "result.txt").write_text(RESULT, encoding="utf-8")
            (run_dir / "report-1.txt").write_text(REPORT, encoding="utf-8")
            (run_dir / "herdr-doctor.json").write_text("{}", encoding="utf-8")
            out_dir = Path(tmp) / "site" / "reports" / "smoke" / "linux" / MODULE.HOST / f"{run_dir.name}-{MODULE.FEATURE}"
            composed: list[tuple[str, Path]] = []

            def fake_compose(template, variables, output):
                composed.append((template.name, output))
                output.write_text(variables.get("body_html", variables.get("pane_src", variables.get("pane_html", ""))), encoding="utf-8")

            with mock.patch.object(MODULE, "compose", side_effect=fake_compose), \
                    mock.patch.object(MODULE, "update_master_report_index") as index, \
                    mock.patch.object(MODULE, "REPO_ROOT", Path(tmp)):
                report = MODULE.render(run_dir, out_dir)
            payload = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual((payload["feature"], payload["host"], payload["platform"], payload["run_id"], payload["status"]),
                             (MODULE.FEATURE, MODULE.HOST, "linux", "20260908T162312Z", "PASS"))
            self.assertEqual(len(payload["cases"]), 7)
            self.assertEqual([name for name, _ in composed],
                             ["inbound-peer-pane.xhtml.j2", "inbound-peer-frame.html.j2", "inbound-peer-review.html.j2"])
            self.assertEqual({p.name for _, p in composed},
                             {f"{MODULE.HOST}-{MODULE.FEATURE}.xhtml", f"{MODULE.FEATURE}.html", "index.html"})
            envelope = json.loads((out_dir / "smoke.envelope.json").read_text(encoding="utf-8"))
            self.assertEqual(envelope["report_html"], f"smoke/linux/{MODULE.HOST}/{out_dir.name}/index.html")
            self.assertEqual(envelope["status"], "PASS")
            for name in ("result.txt", "report-1.txt", "herdr-doctor.json"):
                self.assertEqual((out_dir / name).read_bytes(), (run_dir / name).read_bytes())
            index.assert_called_once()


if __name__ == "__main__":
    unittest.main()
