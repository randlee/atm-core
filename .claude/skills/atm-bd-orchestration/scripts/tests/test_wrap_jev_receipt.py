"""wrap_jev_receipt.py turns raw jev_client.py stdout into a reply whose receipts pass through unchanged."""
import importlib.machinery
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from jev_receipts import KEY, client, env

SCRIPTS = Path(__file__).resolve().parents[1]
SCRIPT = SCRIPTS / "wrap_jev_receipt.py"
_loader = importlib.machinery.SourceFileLoader("sanity_merge_for_wrap", str(SCRIPTS / "sanity-merge"))
_spec = importlib.util.spec_from_loader(_loader.name, _loader)
assert _spec is not None
merge = importlib.util.module_from_spec(_spec)
_loader.exec_module(merge)

FINDING = {"kind": "skipped", "file": "crates/x/src/lib.rs", "line": 3, "issue": "no 503 test"}
ASSIGNMENT = {"sanity_bead": "p-d-4-sanity", "dev_bead": {"id": "p-d-4", "title": "d-4: x"},
              "deliverable": {"number": 2, "text": "Add the 503 test."}, "deliverables_total": 3,
              "commit": "a" * 40}


def answer(choice, response_id=None):
    return {"model": client.MODEL, "id": response_id, "answers": {"written": {
        "type": "choice", "choice": choice, "confidence": 0.9,
        "probabilities": {"yes": 0.9, "no": 0.1} if choice == "yes" else {"yes": 0.1, "no": 0.9}}}}


def single_stdout(choice):
    """What `jev_client.py --assignment` prints for one request, built by the client's own functions."""
    request = client.written_request("Add the 503 test.", "diff")
    response = answer(choice, "resp-1")
    return json.dumps({"success": True, "data": {**response, "receipt": client.receipt(request, response, KEY)},
                       "error": None, "elapsed_s": 4.2})


def split_stdout(choices):
    requests = [client.written_request("Add the 503 test.", f"part {i}", part=(f"{i} of {len(choices)}", ["a.py"]))
                for i in range(1, len(choices) + 1)]
    responses = [answer(choice, f"r{i}") for i, choice in enumerate(choices, 1)]
    return json.dumps({"success": True, "data": client.parts(requests, responses, KEY), "error": None, "elapsed_s": 7.5})


class WrapJevReceipt(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.assignment = self.write("assignment.json", "```json\n" + json.dumps(ASSIGNMENT) + "\n```\n")

    def write(self, name, value):
        path = self.dir / name
        path.write_text(value if isinstance(value, str) else json.dumps(value))
        return path

    def run_wrap(self, stdout_text=None, findings=None, final=None, reply=None, extra=(), environ=None):
        argv = [sys.executable, str(SCRIPT), "--assignment", str(self.assignment)]
        if stdout_text is not None:
            argv += ["--client-output", str(self.write("client.json", stdout_text))]
        if reply is not None:
            argv += ["--reply", str(self.write("reply.json", reply))]
        if findings is not None:
            argv += ["--findings", str(self.write("findings.json", findings))]
        if final is not None:
            argv += ["--final", str(self.write("final.json", final))]
        return subprocess.run([*argv, *extra], capture_output=True, text=True, env=environ or env())

    def wrapped(self, *args, **kwargs):
        done = self.run_wrap(*args, **kwargs)
        self.assertEqual(done.returncode, 0, done.stderr)
        return json.loads(done.stdout)

    def refused(self, check, *args, **kwargs):
        done = self.run_wrap(*args, **kwargs)
        self.assertEqual((done.returncode, done.stdout), (1, ""), done.stderr)
        self.assertTrue(done.stderr.startswith(f"{check}: "), done.stderr)
        self.assertIn("Fix: ", done.stderr)
        return done.stderr

    def assert_merge_accepts(self, reply):
        screened, rejected = merge.screen_jev_results([reply], merge.load_client(merge.CLIENT))
        self.assertEqual(rejected, [])

    def test_single_receipt_passes_through_byte_identical(self):
        raw = single_stdout("no")
        reply = self.wrapped(raw, findings=[FINDING])
        receipt = json.loads(raw)["data"]["receipt"]
        self.assertEqual(reply["data"]["jev"], receipt)
        out = self.run_wrap(raw, findings=[FINDING]).stdout
        for field in ("mac", "request_sha256"):
            self.assertIn(f'"{field}": "{receipt[field]}"', out)
        self.assertEqual(reply["data"]["findings"], [FINDING])
        self.assertEqual((reply["data"]["sanity_bead"], reply["data"]["dev_bead"], reply["data"]["deliverable"],
                          reply["data"]["commit_checked"]), ("p-d-4-sanity", "p-d-4", 2, "a" * 40))
        self.assertTrue(client.verify_receipt(reply["data"]["jev"], KEY))
        with patch.dict(os.environ, {"TYPESAFE_API_KEY": KEY}):
            self.assert_merge_accepts(reply)

    def test_split_parts_pass_through_verbatim(self):
        raw = split_stdout(["yes", "yes"])
        reply = self.wrapped(raw)
        self.assertEqual(reply["data"]["jev"], json.loads(raw)["data"]["parts"])
        self.assertEqual(reply["data"]["findings"], [])
        with patch.dict(os.environ, {"TYPESAFE_API_KEY": KEY}):
            self.assert_merge_accepts(reply)

    def test_hand_edited_receipt_field_is_rejected(self):
        for field, value in (("choice", "yes"), ("mac", "0" * 64), ("request_sha256", "f" * 64),
                             ("probabilities", {"yes": 0.2, "no": 0.8})):
            with self.subTest(field=field):
                raw = json.loads(single_stdout("no"))
                raw["data"]["receipt"][field] = value
                self.refused("receipt_mac", json.dumps(raw), findings=[FINDING])

    def test_hand_written_receipt_without_key_match_is_rejected(self):
        self.refused("receipt_mac", single_stdout("no"), findings=[FINDING],
                     environ={**env(), "TYPESAFE_API_KEY": "another-key"})
        self.refused("receipt_key", single_stdout("no"), findings=[FINDING], environ={**env(), "TYPESAFE_API_KEY": ""})

    def test_edited_part_response_is_rejected(self):
        raw = json.loads(split_stdout(["yes", "yes"]))
        raw["data"]["parts"][1]["response"]["answers"]["written"]["choice"] = "no"
        self.refused("part_response", json.dumps(raw))

    def test_choice_not_in_receipts_is_rejected(self):
        final = {"choice": "yes", "reason": "both parts show it", "parts": ["1 of 2", "2 of 2"]}
        self.refused("choice_in_receipts", split_stdout(["no", "no"]), findings=[FINDING], final=final)
        self.refused("choice_in_receipts", single_stdout("no"), final={"choice": "maybe", "reason": "x", "parts": []})

    def test_multi_choice_without_final_selection_is_rejected(self):
        self.refused("final_selection", split_stdout(["yes", "no"]), findings=[FINDING])
        parts = ["1 of 2", "2 of 2"]
        self.refused("final_reason", split_stdout(["yes", "no"]), findings=[FINDING],
                     final={"choice": "no", "reason": " ", "parts": parts})
        self.refused("final_parts", split_stdout(["yes", "no"]), findings=[FINDING],
                     final={"choice": "no", "reason": "part 2 shows the test missing", "parts": ["1 of 2"]})

    def test_multi_choice_with_final_selection_passes(self):
        final = {"choice": "no", "reason": "part 1 yes covers a.py; part 2 no shows the test missing",
                 "parts": ["1 of 2", "2 of 2"]}
        reply = self.wrapped(split_stdout(["yes", "no"]), findings=[FINDING], final=final)
        self.assertEqual(reply["data"]["jev_final"], final)
        with patch.dict(os.environ, {"TYPESAFE_API_KEY": KEY}):
            self.assert_merge_accepts(reply)

    def test_triage_reply_rewrapped_by_dev_sanity(self):
        raw = split_stdout(["yes", "no"])
        pending = self.wrapped(raw, findings=[FINDING], extra=("--for-triage",))
        self.assertNotIn("jev_final", pending["data"])
        self.assertEqual(pending["data"]["jev"], json.loads(raw)["data"]["parts"])
        with patch.dict(os.environ, {"TYPESAFE_API_KEY": KEY}):
            screened, rejected = merge.screen_jev_results([pending], merge.load_client(merge.CLIENT))
        self.assertEqual(len(rejected), 1)
        final = {"choice": "yes", "reason": "part 1 yes; part 2 no is a.py rename, delivered", "parts": ["1 of 2", "2 of 2"]}
        reply = self.wrapped(reply=pending, findings=[], final=final)
        self.assertEqual((reply["data"]["jev"], reply["data"]["findings"], reply["data"]["jev_final"]),
                         (pending["data"]["jev"], [], final))
        with patch.dict(os.environ, {"TYPESAFE_API_KEY": KEY}):
            self.assert_merge_accepts(reply)
        self.refused("for_triage", split_stdout(["no", "no"]), findings=[FINDING], extra=("--for-triage",))

    def test_findings_shape_violations_are_rejected(self):
        cases = (
            ("yes", [FINDING]),
            ("no", []),
            ("no", [FINDING, FINDING]),
            ("no", [{**FINDING, "kind": "error"}]),
            ("no", [{k: v for k, v in FINDING.items() if k != "file"}]),
            ("no", [{**FINDING, "line": 0}]),
            ("no", [{**FINDING, "line": True}]),
            ("no", [{**FINDING, "issue": ""}]),
            ("no", [{**FINDING, "depends_on": [{"deliverable": 9, "file": "a.py", "line": 1}]}]),
            ("no", {"kind": "skipped"}),
        )
        for choice, findings in cases:
            with self.subTest(choice=choice, findings=findings):
                self.refused("findings_shape", single_stdout(choice), findings=findings)
        self.refused("findings_shape", single_stdout("no"))

    def test_client_failure_is_copied_verbatim_with_deliverable(self):
        error = client.failure(client.JevError("SANITY.JEV_UNAVAILABLE", "Jev connection failed or timed out", True))["error"]
        self.assertTrue(client.is_client_message(error["message"]))
        reply = self.wrapped(json.dumps({"success": False, "data": None, "error": error}))
        self.assertEqual(reply, {"success": False, "data": None, "error": {**error, "deliverable": 2}})
        edited = {**error, "message": "Jev was down, I think"}
        self.refused("client_message", json.dumps({"success": False, "data": None, "error": edited}))

    def test_client_elapsed_s_is_copied_unchanged(self):
        self.assertEqual(self.wrapped(single_stdout("no"), findings=[FINDING])["elapsed_s"], 4.2)
        pending = self.wrapped(split_stdout(["yes", "no"]), findings=[FINDING], extra=("--for-triage",))
        self.assertEqual(pending["elapsed_s"], 7.5)
        final = {"choice": "yes", "reason": "part 1 yes; part 2 no is a rename, delivered", "parts": ["1 of 2", "2 of 2"]}
        self.assertEqual(self.wrapped(reply=pending, findings=[], final=final)["elapsed_s"], 7.5)
        error = client.failure(client.JevError("SANITY.JEV_UNAVAILABLE", "Jev connection failed or timed out", True))["error"]
        failed = self.wrapped(json.dumps({"success": False, "data": None, "error": error, "elapsed_s": 0.0}))
        self.assertEqual(failed["elapsed_s"], 0.0)
        legacy = json.loads(single_stdout("yes"))
        del legacy["elapsed_s"]
        self.assertNotIn("elapsed_s", self.wrapped(json.dumps(legacy)))
        for bad in (-1, True, "4.2"):
            with self.subTest(elapsed_s=bad):
                self.refused("client_elapsed_s", json.dumps({**json.loads(single_stdout("yes")), "elapsed_s": bad}))
        with patch.dict(os.environ, {"TYPESAFE_API_KEY": KEY}):
            self.assert_merge_accepts(self.wrapped(single_stdout("no"), findings=[FINDING]))

    def test_real_client_envelope_is_accepted(self):
        """The envelope jev_client.evaluation_result builds, elapsed_s included, wraps cleanly."""
        args = type("Args", (), {"startup": False, "request": None, "assignment": self.dir / "missing.json"})()
        result = client.evaluation_result(args, clock=iter([0.0, 1.0]).__next__)
        self.assertIn("elapsed_s", result)
        reply = self.wrapped(json.dumps(result))
        self.assertEqual((reply["success"], reply["elapsed_s"], reply["error"]["deliverable"]),
                         (False, result["elapsed_s"], 2))

    def test_not_client_output_is_rejected(self):
        self.refused("client_output_unreadable", "not json")
        self.refused("client_envelope", json.dumps({"data": {}}))
        receipt = json.loads(single_stdout("yes"))["data"]["receipt"]
        self.refused("client_data", json.dumps({"success": True, "data": {"jev": receipt}, "error": None}))


if __name__ == "__main__":
    unittest.main()
