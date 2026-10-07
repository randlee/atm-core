"""Transport/contract failures must not become a successful sanity check."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import MagicMock, patch

spec = importlib.util.spec_from_file_location("jev_client", Path(__file__).parents[1] / "jev_client.py")
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)


def reply():
    return {"model": client.MODEL, "answers": {"startup": {
        "type": "choice", "choice": "ready", "confidence": 0.99,
        "probabilities": {"ready": 1.0, "other": 0.0},
    }}, "usage": {"input_tokens": 1, "output_tokens": 1}}


class JevClientTests(unittest.TestCase):
    def test_missing_key_no_network(self):
        with patch.dict(client.os.environ, {}, clear=True), patch.object(client.http.client, "HTTPSConnection") as http:
            with self.assertRaises(client.JevError):
                client.evaluate(client.startup_request())
            http.assert_not_called()

    def startup(self, argv, team, daemon, environ=None):
        """Run the startup probe without a key; `atm escalation list` answers with `team` / `daemon` recipients."""
        def run(cmd, **kwargs):
            listed = {"--team": team} if "--team" in cmd else {"": daemon}
            recipients = next(iter(listed.values()))
            return MagicMock(returncode=0 if recipients is not None else 1, stdout=json.dumps({"recipients": recipients or []}))
        with patch.dict(client.os.environ, environ if environ is not None else {"ATM_TEAM": "t"}, clear=True), \
             patch.object(client.subprocess, "run", side_effect=run) as send:
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                rc = client.main(argv)
        sends = [(c.args[0][2], c.kwargs["input"]) for c in send.call_args_list if c.args[0][:2] == ["atm", "send"]]
        return rc, json.loads(output.getvalue()), sends

    def test_startup_announces_to_escalation_recipients_else_lead(self):
        rc, result, sends = self.startup(["--startup", "--announce", "--lead", "appointed-lead"], ["oversight@team"], ["daemon@host"])
        self.assertEqual(rc, 2)
        self.assertTrue(result["error"]["recoverable"])
        self.assertEqual(set(result["error"]), {"code", "message", "recoverable", "suggested_action"})
        self.assertEqual([to for to, _ in sends], ["oversight@team"])
        self.assertIn(result["error"]["message"], sends[0][1])
        _, _, sends = self.startup(["--startup", "--announce", "--lead", "appointed-lead"], [], ["daemon@host"])
        self.assertEqual([to for to, _ in sends], ["daemon@host"])
        _, _, sends = self.startup(["--startup", "--announce", "--lead", "appointed-lead"], [], None)
        self.assertEqual([to for to, _ in sends], ["appointed-lead"])
        self.assertTrue(sends[0][1].endswith("No escalation recipient is set."))
        _, _, sends = self.startup(["--startup", "--announce", "--lead", "appointed-lead"], None, ["daemon@host"], environ={})
        self.assertEqual([to for to, _ in sends], ["daemon@host"])

    def test_startup_without_announce_sends_nothing(self):
        rc, _, sends = self.startup(["--startup"], ["oversight@team"], ["daemon@host"])
        self.assertEqual((rc, sends), (2, []))
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            client.main(["--startup", "--announce"])

    def test_error_announces_a_child_failure_verbatim_without_probing(self):
        error = "SANITY.JEV_UNAVAILABLE: Jev HTTP 503; response body withheld"
        with patch.object(client.http.client, "HTTPSConnection") as http:
            rc, result, sends = self.startup(["--announce", "--error", error, "--lead", "appointed-lead"], ["oversight@team"], ["daemon@host"],
                                             environ={"ATM_TEAM": "t", "TYPESAFE_API_KEY": "test-only-key"})
            http.assert_not_called()
        self.assertEqual((rc, result), (0, {"success": True, "data": {"announced": error}, "error": None}))
        self.assertEqual([to for to, _ in sends], ["oversight@team"])
        self.assertIn(error, sends[0][1])
        _, _, sends = self.startup(["--announce", "--error", error, "--lead", "appointed-lead"], [], None)
        self.assertEqual([to for to, _ in sends], ["appointed-lead"])
        self.assertIn(error, sends[0][1])
        self.assertTrue(sends[0][1].endswith("No escalation recipient is set."))
        for argv in (["--error", error], ["--announce", "--error", "", "--lead", "appointed-lead"],
                     ["--announce", "--error", error], ["--startup", "--error", error, "--announce", "--lead", "appointed-lead"],
                     ["--request", "r.json", "--announce", "--lead", "appointed-lead"]):
            with self.subTest(argv=argv), self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                client.main(argv)

    def test_successful_transport(self):
        conn = MagicMock()
        conn.getresponse.return_value.status = 200
        conn.getresponse.return_value.read.return_value = json.dumps(reply()).encode()
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), patch.object(client.http.client, "HTTPSConnection", return_value=conn):
            self.assertEqual(client.evaluate(client.startup_request()), reply())
        self.assertEqual(conn.request.call_args.args[:2], ("POST", "/v1/systemone"))
        conn.close.assert_called_once()

    def test_auth_error_does_not_echo_body_or_key(self):
        conn = MagicMock()
        conn.getresponse.return_value.status = 401
        conn.getresponse.return_value.read.return_value = b"test-only-key server details"
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), patch.object(client.http.client, "HTTPSConnection", return_value=conn):
            with self.assertRaises(client.JevError) as raised:
                client.evaluate(client.startup_request())
        self.assertNotIn("test-only-key", str(raised.exception))
        self.assertFalse(raised.exception.recoverable)
        self.assertEqual(conn.request.call_count, 1)

    def test_retry_is_bounded(self):
        conn = MagicMock()
        conn.getresponse.return_value.status = 429
        conn.getresponse.return_value.read.return_value = b"{}"
        conn.getresponse.return_value.getheader.return_value = "0"
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), patch.object(client.http.client, "HTTPSConnection", return_value=conn), patch.object(client.time, "sleep"):
            with self.assertRaises(client.JevError):
                client.evaluate(client.startup_request())
        self.assertEqual(conn.request.call_count, 2)

    def test_bad_answer_shapes(self):
        for change in [lambda r: r.update(model="wrong"),
                       lambda r: r.update(answers={}),
                       lambda r: r["answers"]["startup"].update(choice={}),
                       lambda r: r["answers"]["startup"].update(confidence=float("nan")),
                       lambda r: r["answers"]["startup"].update(probabilities={"ready": 0.1, "other": 0.1})]:
            value = reply()
            change(value)
            with self.assertRaises(client.JevError):
                client.validate_response(value, client.startup_request())

    def test_oversize_request_abstains(self):
        request = client.startup_request()
        request["state"] = "x" * client.MAX_REQUEST_BYTES
        with self.assertRaises(client.JevError) as raised:
            client.validate_request(request)
        self.assertEqual(raised.exception.code, "SANITY.JEV_INCONCLUSIVE")

    def test_finite_request_values_only(self):
        request = client.startup_request()
        request["state"] = {"bad": float("inf")}
        with self.assertRaises(client.JevError):
            client.validate_request(request)

    def test_request_prints_a_receipt_only_the_call_can_write(self):
        request = {"model": client.MODEL, "state": {"deliverable": "d", "evidence": "e"}, "questions": {"written": {
            "type": "choice", "instructions": "Delivered?", "criteria": {"yes": "delivered", "no": "not delivered"}}}}
        response = {"model": client.MODEL, "id": "resp-1", "answers": {"written": {
            "type": "choice", "choice": "no", "confidence": 0.9, "probabilities": {"yes": 0.1, "no": 0.9}}}}
        conn = MagicMock()
        conn.getresponse.return_value.status = 200
        conn.getresponse.return_value.read.return_value = json.dumps(response).encode()
        import tempfile
        with tempfile.NamedTemporaryFile("w", suffix=".json") as file:
            json.dump(request, file)
            file.flush()
            output = io.StringIO()
            with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                 patch.object(client.http.client, "HTTPSConnection", return_value=conn), contextlib.redirect_stdout(output):
                self.assertEqual(client.main(["--request", file.name]), 0)
        receipt = json.loads(output.getvalue())["data"]["receipt"]
        self.assertEqual({k: receipt[k] for k in ("model", "question", "choice", "probabilities", "response_id")},
                         {"model": client.MODEL, "question": "written", "choice": "no",
                          "probabilities": {"yes": 0.1, "no": 0.9}, "response_id": "resp-1"})
        self.assertEqual(receipt["request_sha256"], client.hashlib.sha256(client.canonical(request)).hexdigest())
        self.assertTrue(client.verify_receipt(receipt, "test-only-key"))
        self.assertFalse(client.verify_receipt(receipt, "another-key"))
        self.assertFalse(client.verify_receipt(dict(receipt, choice="yes"), "test-only-key"))
        self.assertFalse(client.verify_receipt({k: v for k, v in receipt.items() if k != "mac"}, "test-only-key"))

    def test_assignment_request_carries_the_deliverable_verbatim_and_the_committed_diff(self):
        import subprocess, tempfile
        with tempfile.TemporaryDirectory() as tmp:
            def git(*args):
                return subprocess.run(["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", *args],
                                      check=True, capture_output=True, text=True).stdout.strip()
            git("init", "-q")
            Path(tmp, "server.py").write_text("bind()\ndrop()\nrebind()\n")
            Path(tmp, "notes.md").write_text("context note\n")
            git("add", "-A"); git("commit", "-qm", "base")
            base = git("rev-parse", "HEAD")
            Path(tmp, "server.py").write_text("bind()\nhand_off_fd()\n")
            git("commit", "-qam", "fix")
            commit = git("rev-parse", "HEAD")
            assignment = {"deliverable": {"number": 1, "text": "Keep the listener open and hand its fd to the child."},
                          "worktree_path": tmp, "commit": commit, "base_sha": base,
                          "changed_files": ["server.py"], "context": ["notes.md"]}
            request = client.assignment_request(assignment)
            self.assertEqual(request["state"]["deliverable"], assignment["deliverable"]["text"])
            self.assertIn("+hand_off_fd()", request["state"]["evidence"])
            self.assertIn("-rebind()", request["state"]["evidence"])
            self.assertEqual(request["state"]["context"], {"notes.md": "context note\n"})
            client.validate_request(request)

            response = {"model": client.MODEL, "answers": {"written": {
                "type": "choice", "choice": "yes", "confidence": 0.9, "probabilities": {"yes": 0.95, "no": 0.05}}}}
            conn = MagicMock()
            conn.getresponse.return_value.status = 200
            conn.getresponse.return_value.read.return_value = json.dumps(response).encode()
            file = Path(tmp, "..", Path(tmp).name + "-assignment.json").resolve()
            file.write_text(json.dumps(assignment))
            try:
                output = io.StringIO()
                with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                     patch.object(client.http.client, "HTTPSConnection", return_value=conn), contextlib.redirect_stdout(output):
                    self.assertEqual(client.main(["--assignment", str(file)]), 0)
                sent = json.loads(conn.request.call_args.kwargs["body"])
                self.assertEqual(sent, request)
                receipt = json.loads(output.getvalue())["data"]["receipt"]
                self.assertEqual(receipt["request_sha256"], client.hashlib.sha256(client.canonical(request)).hexdigest())
                self.assertTrue(client.verify_receipt(receipt, "test-only-key"))

                for broken, message in (({**assignment, "commit": "0" * 40}, "Committed evidence unreadable at the pinned commits"),
                                        ({k: v for k, v in assignment.items() if k != "base_sha"},
                                         "Assignment lacks deliverable text, worktree, commits or file lists")):
                    file.write_text(json.dumps(broken))
                    output = io.StringIO()
                    with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), contextlib.redirect_stdout(output):
                        self.assertEqual(client.main(["--assignment", str(file)]), 2)
                    error = json.loads(output.getvalue())["error"]
                    self.assertEqual((error["code"], error["message"]), ("VALIDATION.INPUT", message))
            finally:
                file.unlink()

    def test_oversize_assignment_tells_the_agent_to_drop_context_and_retry(self):
        import subprocess, tempfile
        with tempfile.TemporaryDirectory() as tmp:
            def git(*args):
                return subprocess.run(["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", *args],
                                      check=True, capture_output=True, text=True).stdout.strip()
            git("init", "-q")
            Path(tmp, "server.py").write_text("bind()\n")
            Path(tmp, "big.md").write_text("x" * client.MAX_REQUEST_BYTES)
            git("add", "-A"); git("commit", "-qm", "base")
            base = git("rev-parse", "HEAD")
            Path(tmp, "server.py").write_text("bind()\nhand_off_fd()\n")
            git("commit", "-qam", "fix")
            assignment = {"deliverable": {"number": 1, "text": "Hand the fd to the child."}, "worktree_path": tmp,
                          "commit": git("rev-parse", "HEAD"), "base_sha": base,
                          "changed_files": ["server.py"], "context": ["big.md"]}
            file = Path(tmp, "..", Path(tmp).name + "-assignment.json").resolve()
            file.write_text(json.dumps(assignment))
            try:
                output = io.StringIO()
                with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                     patch.object(client.http.client, "HTTPSConnection") as https, contextlib.redirect_stdout(output):
                    self.assertEqual(client.main(["--assignment", str(file)]), 2)
                https.assert_not_called()
                error = json.loads(output.getvalue())["error"]
                self.assertEqual(error["code"], "SANITY.JEV_INCONCLUSIVE")
                self.assertTrue(error["recoverable"] and client.is_client_message(error["message"]))
                size = len(json.dumps(client.assignment_request(assignment), allow_nan=False).encode())
                self.assertIn(f"The request is {size} bytes. Reduce context and retry", error["suggested_action"])
                self.assertIn("`context`", error["suggested_action"])
                client.validate_request(client.assignment_request({**assignment, "context": []}))  # the retry fits
            finally:
                file.unlink()

    def test_each_known_error_returns_its_fix(self):
        cases = {
            ("SANITY.JEV_UNAVAILABLE", "Jev connection failed or timed out", True): "wait 60 seconds and run the same command once more",
            ("SANITY.JEV_UNAVAILABLE", "Jev HTTP 503; response body withheld", True): "wait 60 seconds",
            ("SANITY.JEV_UNAVAILABLE", "Jev HTTP 401; response body withheld", False): "Report this error unchanged",
            ("VALIDATION.INPUT", "Committed evidence unreadable at the pinned commits", False): "git -C <worktree_path> fetch origin",
            ("VALIDATION.INPUT", "Request file unavailable or invalid JSON", False): "Write the file as the JSON object",
            ("SANITY.JEV_RESPONSE_INVALID", "Invalid Choice answer", False): "Report this error unchanged",
        }
        for (code, message, recoverable), fix in cases.items():
            with self.subTest(message=message):
                self.assertIn(fix, client.failure(client.JevError(code, message, recoverable))["error"]["suggested_action"])

    def test_every_failure_message_the_client_writes_is_recognised(self):
        source = (Path(__file__).parents[1] / "jev_client.py").read_text()
        import re
        literals = re.findall(r'JevError\("[A-Z_.]+", f?"([^"]+)"', source)
        self.assertGreaterEqual(len(literals), 15)
        for literal in literals:
            with self.subTest(message=literal):
                self.assertTrue(client.is_client_message(literal.replace("{status}", "503")))
        self.assertFalse(client.is_client_message("Selected model is at capacity"))


if __name__ == "__main__":
    unittest.main()
