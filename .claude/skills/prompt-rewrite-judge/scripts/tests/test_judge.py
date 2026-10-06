import importlib.util, io, json, sys, tempfile, unittest
from contextlib import redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))
import judge

CLIENT = Path(__file__).resolve().parents[3] / "atm-bd-orchestration/scripts/jev_client.py"


def answer(choice, probs):
    return {"type": "choice", "choice": choice, "confidence": 0.5, "probabilities": probs}


def answers(drop=None):
    out = {f"keep_{i}": answer("no", {"yes": 0.1, "no": 0.9}) if i == drop else answer("yes", {"yes": 0.95, "no": 0.05})
           for i in (1, 2)}
    out["changes_rule"] = answer("no", {"no": 0.57, "yes": 0.43})
    out["unnecessary"] = answer("none", {"none": 0.8, "some": 0.15, "mostly": 0.05})
    out["ambiguous"] = answer("yes", {"no": 0.4, "yes": 0.6})
    out["clearer_than_original"] = answer("same", {"yes": 0.2, "same": 0.7, "no": 0.1})
    return out


FAKE = """import json, sys
from pathlib import Path
req = json.loads(Path(sys.argv[2]).read_text())
state = Path(sys.argv[2]).with_suffix(".calls")
n = int(state.read_text()) + 1 if state.exists() else 1
state.write_text(str(n))
if "CRASH" in req["state"]["candidate"]:
    sys.exit("Traceback: quota exceeded")
if "FAIL" in req["state"]["candidate"] or ("FLAKY" in req["state"]["candidate"] and n == 1):
    print(json.dumps({"success": False, "data": None, "error": {"code": "SANITY.JEV_UNAVAILABLE", "message": "TYPESAFE_API_KEY is missing"}}))
    sys.exit(2)
else:
    print(json.dumps({"success": True, "data": {"answers": ANSWERS}}))
"""


class RequestTests(unittest.TestCase):
    def test_one_keep_question_per_requirement_and_the_four_verdicts(self):
        req = judge.request(["Run tests.", "Push."], "orig", "cand")
        self.assertEqual(list(req["questions"]), ["keep_1", "keep_2", "changes_rule", "unnecessary", "ambiguous", "clearer_than_original"])
        self.assertIn("Requirement: Push.", req["questions"]["keep_2"]["instructions"])
        self.assertEqual(set(req["questions"]["keep_1"]["criteria"]), {"yes", "no"})
        self.assertEqual(set(req["questions"]["changes_rule"]["criteria"]), {"no", "yes"})
        self.assertEqual(req["state"], {"requirements": ["Run tests.", "Push."], "original": "orig", "candidate": "cand"})

    def test_request_is_accepted_by_the_package_jev_client(self):
        spec = importlib.util.spec_from_file_location("jev_client", CLIENT)
        client = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(client)
        req = judge.request(["Run tests."], "orig", "cand")
        self.assertEqual(req["model"], client.MODEL)
        client.validate_request(req)


class ReduceTests(unittest.TestCase):
    def test_reports_drops_with_p_no_and_choices_with_their_own_probability(self):
        v = judge.reduce(answers(drop=2))
        self.assertEqual(v["dropped"], [(2, 0.9)])
        self.assertEqual(v["changes_rule"], ("no", 0.57))
        self.assertEqual(v["ambiguous"], ("yes", 0.6))
        self.assertEqual(judge.row("r1", 10, v), "r1 | 10 | R2(0.90) | no(0.57) | none(0.80) | yes(0.60) | same(0.70)")

    def test_drops_are_in_requirement_order(self):
        a = answers()
        for i in (10, 2):
            a[f"keep_{i}"] = answer("no", {"yes": 0.0, "no": 1.0})
        self.assertEqual([n for n, _ in judge.reduce(a)["dropped"]], [2, 10])

    def test_no_drop_and_failed_row(self):
        self.assertEqual(judge.reduce(answers())["dropped"], [])
        self.assertEqual(judge.row("r2", 7, None, {"exit": 2}), 'r2 | 7 | JEV FAILED {"exit": 2}')


class MainTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.d = Path(tmp.name)
        (self.d / "key.json").write_text(json.dumps(["Run tests.", "Push."]))
        (self.d / "original.txt").write_text("Run tests. Push.")
        (self.d / "client.py").write_text("ANSWERS = " + repr(answers(drop=1)) + "\n" + FAKE)

    def main(self, *cands):
        paths = []
        for name, text in cands:
            (self.d / f"{name}.txt").write_text(text)
            paths.append(str(self.d / f"{name}.txt"))
        buf = io.StringIO()
        with redirect_stdout(buf):
            code = judge.main(["--key", str(self.d / "key.json"), "--original", str(self.d / "original.txt"),
                               "--out", str(self.d / "jev"), "--client", str(self.d / "client.py"), *paths])
        return code, buf.getvalue().splitlines()

    def test_rerun_once_then_record_failure(self):
        code, lines = self.main(("ok", "Run tests."), ("flaky", "FLAKY push"), ("bad", "FAIL"))
        self.assertEqual(code, 2)
        self.assertEqual(lines[0], judge.HEADER)
        self.assertEqual(lines[1], "ok | 2 | R1(0.90) | no(0.57) | none(0.80) | yes(0.60) | same(0.70)")
        self.assertTrue(lines[2].startswith("flaky | 2 | R1(0.90)"))
        self.assertEqual(lines[3], "bad | 1 | JEV FAILED " + json.dumps({"exit": 2, "error": {
            "code": "SANITY.JEV_UNAVAILABLE", "message": "TYPESAFE_API_KEY is missing"}, "stdout": None, "stderr": ""}))
        self.assertEqual((self.d / "jev/bad.req.calls").read_text(), "2")
        self.assertFalse((self.d / "jev/bad.json").exists())
        self.assertEqual(json.loads((self.d / "jev/ok.req.json").read_text())["state"]["candidate"], "Run tests.")

    def test_client_crash_records_its_exit_code_stdout_and_stderr_verbatim(self):
        code, lines = self.main(("crash", "CRASH"))
        self.assertEqual(code, 2)
        self.assertEqual(lines[1], "crash | 1 | JEV FAILED " + json.dumps(
            {"exit": 1, "error": None, "stdout": "", "stderr": "Traceback: quota exceeded\n"}))
        self.assertEqual((self.d / "jev/crash.req.calls").read_text(), "2")

    def test_all_judged_exits_zero(self):
        self.assertEqual(self.main(("ok", "Run tests."))[0], 0)

    def test_key_must_be_a_list_of_strings(self):
        (self.d / "key.json").write_text(json.dumps({"R1": "Run tests."}))
        with self.assertRaises(SystemExit):
            self.main(("ok", "x"))

    def test_oversized_request_is_refused_before_any_call(self):
        with self.assertRaises(SystemExit):
            self.main(("big", "x " * (judge.MAX_REQUEST_BYTES // 2 + 1)))
        self.assertFalse((self.d / "jev/big.req.json").exists())


if __name__ == "__main__":
    unittest.main()
