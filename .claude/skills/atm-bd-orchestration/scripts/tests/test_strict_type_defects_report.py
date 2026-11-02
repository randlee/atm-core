"""Regression coverage for concrete post-mortem/report type defects."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(SCRIPTS))
import post_mortem_jev as jev
from test_post_mortem_jev import response


REAL_CLIENT = jev.load_client(SCRIPTS / "jev_client.py")


class FakeClient:
    MODEL = REAL_CLIENT.MODEL
    validate_request = staticmethod(REAL_CLIENT.validate_request)

    @staticmethod
    def evaluate(request):
        answers = {}
        choices = {
            "fix_presence": "fully_present",
            "serious_fix_issues": "no_serious_issue_seen",
            "fix_quality": "adequate",
        }
        for question_id, question in jev.QUESTIONS.items():
            choice = choices[question_id]
            answers[question_id] = {
                "type": "choice",
                "choice": choice,
                "confidence": 0.9,
                "probabilities": {key: 1.0 if key == choice else 0.0 for key in question["criteria"]},
            }
        return {"model": REAL_CLIENT.MODEL, "answers": answers}


class MissingLimitationsTests(unittest.TestCase):
    def setUp(self):
        self.tempdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tempdir.cleanup)
        self.root = Path(self.tempdir.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        for args in (("init", "-q"), ("config", "user.name", "test"), ("config", "user.email", "test@example.invalid")):
            subprocess.run(["git", "-C", str(self.repo), *args], check=True, capture_output=True)
        (self.repo / "code.rs").write_text("fn execute() {}\n")
        subprocess.run(["git", "-C", str(self.repo), "add", "."], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(self.repo), "commit", "-qm", "fixture"], check=True, capture_output=True)
        self.manifest = {
            "finding_id": "f1",
            "phase": "phase-d",
            "integration_sha": jev.resolve_sha(self.repo, "HEAD"),
            "finding_text": "A complete original defect.",
            "acceptance_predicates": [{"id": "o1", "text": "execute exists"}],
            "coverage": {"scope": "whole_finding", "limitations": [], "obligation_ids": ["o1"]},
            "source_selections": [{"path": "code.rs", "role": "production"}],
        }

    def test_missing_limitations_is_rejected_at_prepare_and_evaluation_boundaries(self):
        missing_limitations = copy.deepcopy(self.manifest)
        del missing_limitations["coverage"]["limitations"]
        with self.assertRaises(ValueError):
            jev.prepare(missing_limitations, self.repo, FakeClient)
        self.assertEqual(jev.route(FakeClient.evaluate({}), missing_limitations["coverage"]), "evaluation_error")

        packet = jev.prepare(self.manifest, self.repo, FakeClient)
        packet_json = json.loads(json.dumps(packet))
        del packet_json["state"]["coverage"]["limitations"]
        packet_path = self.root / "missing-limitations.json"
        packet_path.write_text(json.dumps(packet_json))
        row = jev.run([packet_path], FakeClient, self.root / "logs", "phase-d")[0]
        self.assertEqual(row["screening_disposition"], "evaluation_error")
        self.assertIsNotNone(row["error"])


class PostMortemProbabilityTypeTests(unittest.TestCase):
    def test_integer_zero_and_one_are_valid_but_bool_is_not(self):
        reply = response()
        for answer in reply["answers"].values():
            answer["probabilities"] = {
                choice: 1 if choice == answer["choice"] else 0
                for choice in answer["probabilities"]
            }

        coverage = {"scope": "whole_finding", "limitations": [], "obligation_ids": ["o1"]}
        self.assertEqual(jev.route(reply, coverage), "screened_present")

        reply["answers"]["fix_presence"]["probabilities"]["fully_present"] = True
        self.assertEqual(jev.route(reply, coverage), "evaluation_error")


if __name__ == "__main__":
    unittest.main()
